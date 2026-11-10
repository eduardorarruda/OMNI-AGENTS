import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ApiError, chaveDeAtencao, esquecerVersao, precisaDeVoce, request, requestSeMudou, salvarToken, type Conversation, type Painel, type Projects } from "./api";
import { aplicarTema, guardarTema, temaGuardado, type TemaPublicado } from "./tema";
import { usePoll } from "./usePoll";

const CHAVE_VISTAS = "omni-vistas";

/** Pedido de atenção já visto, por conversa. Fica no aparelho: cada celular/navegador tem a sua
 *  caixa de entrada, como num app de mensagens. */
function lerVistas(): Record<string, string> {
  try {
    const salvo = JSON.parse(localStorage.getItem(CHAVE_VISTAS) ?? "{}");
    return salvo && typeof salvo === "object" && !Array.isArray(salvo) ? salvo : {};
  } catch {
    return {};
  }
}

function gravarVistas(vistas: Record<string, string>) {
  try { localStorage.setItem(CHAVE_VISTAS, JSON.stringify(vistas)); } catch { /* vale só nesta aba */ }
}

/** A página está na frente de alguém. Conversa aberta numa aba escondida não foi vista. */
function useVisivel(): boolean {
  const [visivel, setVisivel] = useState(() => typeof document === "undefined" || document.visibilityState !== "hidden");
  useEffect(() => {
    const atualizar = () => setVisivel(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", atualizar);
    return () => document.removeEventListener("visibilitychange", atualizar);
  }, []);
  return visivel;
}

/**
 * Estado que vem do PC: conversas, o que pede atenção, o catálogo de projetos e o tema.
 *
 * Extraído do `MobileApp` porque a página de computador (`src/web`) precisa exatamente do mesmo
 * carregamento — mesma API, mesmo poll, mesmo pareamento. O que muda entre as duas é só o layout.
 */
/** Resposta do `/parear`: pelo Tailscale vem o token direto; pelo link seguro vem um pedido que o
 *  dono precisa aprovar no PC, e o celular pergunta até ter resposta. */
interface RespostaDoPareamento { token?: string; pedido?: string; expira_em_s?: number }

/** Intervalo entre as perguntas "já aprovaram?". */
export const ESPERA_DA_APROVACAO_MS = 2000;

export async function trocarCodigoPorToken(codigo: string, aoAguardar?: () => void): Promise<string> {
  const json = { method: "POST", headers: { "Content-Type": "application/json" } };
  let resposta = await request<RespostaDoPareamento>("/parear", { ...json, body: JSON.stringify({ codigo }) });
  if (resposta.token) return resposta.token;
  if (!resposta.pedido) throw new Error("O PC respondeu de um jeito inesperado. Tente de novo.");
  aoAguardar?.();
  const pedido = resposta.pedido;
  // Folga sobre o prazo do PC: quem decide que expirou é ele (410), isto só evita esperar para sempre.
  const limite = Date.now() + ((resposta.expira_em_s ?? 120) + 30) * 1000;
  while (!resposta.token) {
    if (Date.now() > limite) throw new Error("O pedido expirou sem aprovação. Digite um código novo.");
    await new Promise((pronto) => window.setTimeout(pronto, ESPERA_DA_APROVACAO_MS));
    resposta = await request<RespostaDoPareamento>("/parear/pedido", { ...json, body: JSON.stringify({ pedido }) });
  }
  return resposta.token;
}

export function useDados(aberta: string | null = null) {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [vistas, setVistas] = useState<Record<string, string>>(lerVistas);
  const visivel = useVisivel();
  const [catalog, setCatalog] = useState<Projects | null>(null);
  const [carregado, setCarregado] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Sem acesso = o servidor respondeu 401. Acontece sempre na primeira abertura do app da tela
  // inicial do iPhone, que não enxerga o token que o QR gravou no Safari.
  const [semAcesso, setSemAcesso] = useState(false);
  // Começa pelo último tema recebido: a tela de pareamento e o primeiro quadro já saem com a cara do PC.
  const [tema, setTema] = useState<TemaPublicado | null>(() => temaGuardado());

  useEffect(() => aplicarTema(tema), [tema]);

  /** Engine sem `/painel` (versão anterior): volta para as três rotas antigas e não tenta de novo. */
  const semPainel = useRef(false);
  /** A primeira leitura de cada montagem vem inteira: o ETag guardado é de uma tela que já não
   *  existe, e um 304 aqui deixaria a lista vazia. */
  const temDados = useRef(false);

  const aplicar = useCallback((lista: Conversation[], projetos: Projects | null) => {
    // Normaliza na entrada: um engine mais antigo ou uma resposta truncada deixava `projects`
    // indefinido, e um `.find` adiante derrubava a tela inteira — sem lista, sem conversa, sem
    // mensagem de erro. Confiar no formato aqui é o que deixa o resto simples.
    const novoTema = projetos?.theme && typeof projetos.theme === "object" ? projetos.theme : null;
    setCatalog({
      published_at_ms: Number(projetos?.published_at_ms) || 0,
      projects: Array.isArray(projetos?.projects) ? projetos.projects : [],
      agents: Array.isArray(projetos?.agents) ? projetos.agents : [],
      profiles: Array.isArray(projetos?.profiles) ? projetos.profiles : [],
      theme: novoTema,
    });
    if (novoTema) {
      guardarTema(novoTema);
      // Compara por valor: objeto novo a cada poll reaplicaria o tema (e o `matchMedia`) à toa.
      setTema((atual) => (JSON.stringify(atual) === JSON.stringify(novoTema) ? atual : novoTema));
    }
    const conversas = Array.isArray(lista) ? lista : [];
    setConversations(conversas);
  }, []);

  // Abrir a conversa (com a página na frente) marca o pedido atual como visto. Só volta para
  // "Precisa de você" quando o agente terminar outro turno ou pedir aprovação — uma entrada por
  // conversa, nunca uma por evento, e nada reaparece sem algo novo ter acontecido.
  const conversaAberta = aberta && visivel ? conversations.find((item) => item.id === aberta) : undefined;
  const chaveAberta = conversaAberta && precisaDeVoce(conversaAberta) ? chaveDeAtencao(conversaAberta) : null;
  useEffect(() => {
    if (!aberta || chaveAberta === null) return;
    setVistas((atual) => {
      if (atual[aberta] === chaveAberta) return atual;
      // Poda o que não existe mais no PC, senão o mapa cresce para sempre.
      const ids = new Set(conversations.map((item) => item.id));
      const novas: Record<string, string> = { [aberta]: chaveAberta };
      for (const [id, chave] of Object.entries(atual)) if (ids.has(id) && id !== aberta) novas[id] = chave;
      gravarVistas(novas);
      return novas;
    });
  }, [aberta, chaveAberta, conversations]);

  const attention = useMemo(
    () => conversations.filter((item) =>
      precisaDeVoce(item)
      && !(item.id === aberta && visivel)
      && vistas[item.id] !== chaveDeAtencao(item)),
    [conversations, vistas, aberta, visivel]
  );

  const refresh = useCallback(async () => {
    if (!semPainel.current) {
      try {
        // `null` = nada mudou desde a última leitura: a tela fica como está, sem re-renderizar.
        if (!temDados.current) esquecerVersao("/painel");
        const painel = await requestSeMudou<Painel>("/painel");
        // Resposta fora do formato (proxy, engine estranho): melhor as rotas antigas que tela vazia.
        if (painel === null || Array.isArray(painel?.conversations)) {
          if (painel) { aplicar(painel.conversations, painel.catalog); temDados.current = true; }
          setCarregado(true);
          setError(null);
          return;
        }
        semPainel.current = true;
      } catch (reason) {
        if (!(reason instanceof ApiError && reason.status === 404)) throw reason;
        semPainel.current = true;
      }
    }
    const [all, projetos] = await Promise.all([request<Conversation[]>("/conversas"), request<Projects>("/projetos")]);
    aplicar(all, projetos);
    setCarregado(true);
    setError(null);
  }, [aplicar]);

  usePoll(
    refresh,
    (reason) => {
      if (reason instanceof ApiError && reason.status === 401) {
        setSemAcesso(true);
        return;
      }
      setError(String(reason));
    },
    [refresh]
  );

  const parear = useCallback(
    async (codigo: string, aoAguardar?: () => void) => {
      const token = await trocarCodigoPorToken(codigo, aoAguardar);
      salvarToken(token);
      setSemAcesso(false);
      setError(null);
      await refresh();
    },
    [refresh]
  );

  const atualizar = useCallback(() => refresh().catch((reason) => setError(String(reason))), [refresh]);

  return { conversations, attention, catalog, carregado, error, semAcesso, refresh, atualizar, parear, setError };
}

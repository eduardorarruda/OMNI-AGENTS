import { useEffect, useMemo, useState } from "react";
import { AgentIcon } from "../../components/ui/AgentIcon";
import { SimboloDoProjeto } from "../../components/EscolherIcone";
import { haQuantoTempo } from "../../components/ConversaStatus";
import { ChevronDownIcon, ClockIcon } from "../../components/ui/PixelIcon";
import { baseName } from "../files/filesService";
import { useProjetoDaPasta } from "../projects/projetosContext";
import { filterHistory, listHistory, pastaDoProjeto, samePath, type HistoryEntry } from "./historyService";
import { Input } from "../../components/ui/Input";
import { listConversations, type Conversation } from "../terminal/terminalService";

const QUANTAS = 6;

/** As conversas mais recentes do Histórico (Claude e Codex, todos os projetos) que não estão
 *  abertas agora: um mini histórico na lateral para voltar a uma delas com um clique. */
export function escolherRecentes(entradas: HistoryEntry[], abertas: ReadonlySet<string>, quantas = QUANTAS): HistoryEntry[] {
  return entradas
    .filter((entrada) => entrada.cwd && !abertas.has(entrada.session_id))
    .sort((a, b) => b.updated_at_ms - a.updated_at_ms)
    .slice(0, quantas);
}

/** Todas as conversas guardadas de um projeto, da mais recente para a mais antiga. Worktrees e
 *  subpastas contam como do projeto, a não ser que a subpasta seja um projeto aberto também. */
export function conversasDoProjeto(entradas: HistoryEntry[], abertas: ReadonlySet<string>, projeto: string, projetos: readonly string[]): HistoryEntry[] {
  const doProjeto = entradas.filter((entrada) => entrada.cwd && samePath(pastaDoProjeto(entrada.cwd, projetos), projeto));
  return escolherRecentes(doProjeto, abertas, doProjeto.length);
}

const PROVIDERS_DO_HISTORICO = new Set<string>(["claude", "codex", "cursor"]);

/** Une o histórico das CLIs com o índice de conversas do OMNI. O scan só acha conversa com prompt
 *  digitado num `.jsonl` legível; a que o OMNI registrou e ainda não tem transcript (ou cujo
 *  transcript o scan descarta) sumia da lista. Mesma conversa nos dois aparece uma vez: a do
 *  histórico, que tem o primeiro prompt e a data real. Conversa sem nenhuma sessão nativa não tem o
 *  que retomar e fica de fora — enquanto o agente dela está vivo, ela aparece em CHATS. */
export function juntarComIndice(entradas: HistoryEntry[], conversas: readonly Conversation[]): HistoryEntry[] {
  const conhecidas = new Set(entradas.map((entrada) => entrada.session_id));
  const doIndice = conversas.flatMap((conversa): HistoryEntry[] => {
    const comSessao = conversa.segments.filter((segmento) => segmento.external_session_id && PROVIDERS_DO_HISTORICO.has(segmento.provider));
    if (comSessao.some((segmento) => conhecidas.has(segmento.external_session_id!))) return [];
    const ultimo = comSessao.reduce<(typeof comSessao)[number] | null>(
      (maisNovo, segmento) => (!maisNovo || segmento.started_at_ms >= maisNovo.started_at_ms ? segmento : maisNovo), null);
    if (!ultimo) return [];
    return [{
      provider: ultimo.provider as HistoryEntry["provider"],
      profile_id: ultimo.profile_id ?? "",
      profile_name: "",
      session_id: ultimo.external_session_id!,
      title: conversa.title?.trim() || null,
      first_prompt: null,
      cwd: conversa.cwd || null,
      started_at_ms: conversa.created_at_ms,
      updated_at_ms: ultimo.ended_at_ms ?? ultimo.started_at_ms,
      path: ultimo.transcript_path ?? "",
    }];
  });
  return [...entradas, ...doIndice];
}

export function RecentesConversas({ abertas, agora, onRetomar, onVerTodas, projeto = null, projetos = [] }: {
  /** Ids nativos (`external_session_id`) das conversas já abertas: essas não se repetem aqui. */
  abertas: readonly string[];
  agora: number;
  onRetomar: (entrada: HistoryEntry) => Promise<void>;
  onVerTodas: () => void;
  /** Pasta do projeto ativo: com ela, a lista vira todas as conversas desse projeto. */
  projeto?: string | null;
  projetos?: readonly string[];
}) {
  const [entradas, setEntradas] = useState<HistoryEntry[] | null>(null);
  const [aberto, setAberto] = useState(true);
  const [retomando, setRetomando] = useState<string | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [busca, setBusca] = useState("");
  const chave = [...abertas].sort().join(",");

  // Relê quando abre/fecha uma conversa (a que fechou volta para cá). O scan tem cache por arquivo.
  useEffect(() => {
    let vivo = true;
    const indice = listConversations().catch(() => [] as Conversation[]);
    Promise.all([listHistory("claude"), listHistory("codex"), listHistory("cursor")].map((p) => p.catch(() => [] as HistoryEntry[])))
      .then(async (listas) => {
        const conversas = await indice;
        if (vivo) setEntradas(juntarComIndice(listas.flat(), conversas));
      });
    return () => { vivo = false; };
  }, [chave]);

  // Com busca, procura no histórico inteiro (título, prompt, pasta) e mostra um pouco mais.
  const lista = useMemo(() => {
    const jaAbertas = new Set(abertas);
    if (busca.trim()) return escolherRecentes(filterHistory(entradas ?? [], { query: busca, cwd: "", profileId: "" }), jaAbertas, 10);
    return projeto ? conversasDoProjeto(entradas ?? [], jaAbertas, projeto, projetos) : escolherRecentes(entradas ?? [], jaAbertas);
  }, [entradas, chave, busca, projeto, projetos]); // eslint-disable-line react-hooks/exhaustive-deps
  if (entradas !== null && entradas.length === 0) return null;

  async function retomar(entrada: HistoryEntry) {
    setRetomando(entrada.session_id); setErro(null);
    try { await onRetomar(entrada); }
    catch (motivo) { setErro(motivo instanceof Error ? motivo.message : String(motivo)); }
    finally { setRetomando(null); }
  }

  return (
    <div className="space-y-0.5" aria-label="Conversas recentes" role="region">
      <button type="button" data-nav-item aria-expanded={aberto} onClick={() => setAberto((v) => !v)}
        className="lateral-secao flex w-full items-center gap-2 px-3 pt-1 text-[10px] font-medium uppercase tracking-wider text-text-muted focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent">
        <ChevronDownIcon className={`h-3 w-3 shrink-0 transition-transform ${aberto ? "" : "-rotate-90"}`} aria-hidden="true" />
        <ClockIcon className="h-3 w-3 shrink-0" aria-hidden />
        {projeto ? `Conversas do projeto (${lista.length})` : "Recentes"}
      </button>
      {aberto && <div className="space-y-1 pt-0.5">
        <Input size="sm" type="search" value={busca} onChange={(evento) => setBusca(evento.target.value)}
          placeholder="Buscar no histórico…" aria-label="Buscar nas conversas recentes" className="lateral-recente-busca"
          onKeyDown={(evento) => { if (evento.key === "Escape") setBusca(""); }} />
        {entradas !== null && lista.length === 0 && (
          <p className="px-3 py-1.5 text-[11px] text-text-muted">{busca.trim() ? "Nada encontrado." : projeto ? "Nenhuma outra conversa neste projeto." : "Todas as conversas recentes já estão abertas."}</p>
        )}
        {entradas === null
          ? <p className="px-6 py-1.5 text-xs text-text-muted">Lendo o histórico…</p>
          : lista.map((entrada) => <LinhaRecente key={`${entrada.provider}:${entrada.session_id}`} entrada={entrada} agora={agora}
              retomando={retomando === entrada.session_id} desabilitada={retomando !== null} onClick={() => void retomar(entrada)} />)}
        {erro && <p role="alert" className="mx-1 border-l-2 border-danger px-2 py-1 text-[10px] text-danger">{erro}</p>}
        <button type="button" data-nav-item onClick={onVerTodas}
          className="lateral-recente-todas focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent">
          Ver histórico completo
        </button>
      </div>}
    </div>
  );
}

function LinhaRecente({ entrada, agora, retomando, desabilitada, onClick }: {
  entrada: HistoryEntry; agora: number; retomando: boolean; desabilitada: boolean; onClick: () => void;
}) {
  const projeto = useProjetoDaPasta(entrada.cwd);
  const nomeDoProjeto = projeto?.name ?? baseName(entrada.cwd ?? "");
  const titulo = entrada.title?.trim() || entrada.first_prompt?.trim() || "Conversa sem título";
  return (
    <button type="button" data-nav-item disabled={desabilitada} onClick={onClick}
      aria-label={`Retomar ${titulo} (${nomeDoProjeto})`}
      title={`${titulo}\n${entrada.cwd ?? ""}${entrada.profile_name ? ` · ${entrada.profile_name}` : ""}\nClique para retomar`}
      className={`lateral-item lateral-recente group mx-1 flex w-[calc(100%-0.5rem)] items-center gap-2.5 px-2 py-2 text-left ring-1 ring-inset ring-border-subtle/40 transition-colors hover:ring-border-subtle disabled:cursor-wait focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent${retomando ? " is-retomando" : ""}`}>
      <span className="lateral-avatar relative grid h-8 w-8 shrink-0 place-items-center border border-border-subtle">
        <AgentIcon provider={entrada.provider} size={16} />
        <span className="lateral-recente-projeto" aria-hidden>
          <SimboloDoProjeto project={projeto ?? { path: entrada.cwd ?? "" }} className="h-2.5 w-2.5" />
        </span>
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="min-w-0 flex-1 truncate text-xs leading-4 text-text-secondary group-hover:text-text-primary">{titulo}</span>
          <span className="shrink-0 text-[10px] leading-4 tabular-nums text-text-muted">{haQuantoTempo(entrada.updated_at_ms, agora)}</span>
        </span>
        <span className="mt-0.5 flex min-w-0 items-center gap-1 text-[10px] leading-4 text-text-muted">
          <span className="truncate">{retomando ? "abrindo…" : nomeDoProjeto}</span>
          {entrada.profile_name && <><span aria-hidden>·</span><span className="truncate">{entrada.profile_name}</span></>}
        </span>
      </span>
    </button>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import { engineApi } from "../terminal/terminalService";
import { CheckIcon, CloseIcon, ReloadIcon, ShieldIcon, WarningIcon } from "../../components/ui/PixelIcon";

/** Um aparelho que acertou o código do Authy pelo link seguro e espera o dono deixar entrar. */
export interface PedidoDeAparelho { id: string; aparelho: string; ip: string; em_ms: number; expira_em_ms: number }

export interface ListaDePedidos {
  pedidos: PedidoDeAparelho[];
  pareamento_desligado: boolean;
  pausado_ate_ms: number | null;
  alerta: { em_ms: number; texto: string } | null;
}

/** Quanto esperar entre leituras: rápido o bastante para o celular não ficar esperando à toa. */
const INTERVALO_MS = 3000;
/** Engine antigo (sem a rota) ou fora do ar: tenta de novo bem mais devagar. */
const INTERVALO_COM_ERRO_MS = 30_000;

/** "Android · Chrome", "iPhone · Safari"… a partir do `User-Agent`, para o dono reconhecer o aparelho. */
export function descreverAparelho(ua: string): string {
  const sistema = /iPhone/.test(ua) ? "iPhone" : /iPad/.test(ua) ? "iPad" : /Android/.test(ua) ? "Android"
    : /Windows/.test(ua) ? "Windows" : /Mac OS X|Macintosh/.test(ua) ? "Mac" : /Linux/.test(ua) ? "Linux" : null;
  const navegador = /SamsungBrowser/.test(ua) ? "Samsung Internet" : /Edg\//.test(ua) ? "Edge" : /Firefox|FxiOS/.test(ua) ? "Firefox"
    : /OPR\//.test(ua) ? "Opera" : /Chrome|CriOS/.test(ua) ? "Chrome" : /Safari/.test(ua) ? "Safari" : null;
  const partes = [sistema, navegador].filter(Boolean);
  return partes.length ? partes.join(" · ") : "Aparelho desconhecido";
}

async function chamar(metodo: "GET" | "POST", caminho: string): Promise<ListaDePedidos> {
  const resposta = await engineApi(metodo, caminho, metodo === "POST" ? "{}" : null,
    metodo === "POST" ? { headers: [["Content-Type", "application/json"]] } : {});
  const corpo = JSON.parse(resposta.body || "null") as (ListaDePedidos & { error?: string }) | null;
  if (resposta.status >= 400 || !corpo || !Array.isArray(corpo.pedidos)) {
    throw new Error(corpo?.error || `O engine respondeu ${resposta.status}.`);
  }
  return corpo;
}

/** Toast do sistema. Fora do Tauri (testes) falha; o aviso na tela continua valendo. */
async function notificar(titulo: string, corpo: string) {
  try {
    const { isPermissionGranted, requestPermission, sendNotification } = await import("@tauri-apps/plugin-notification");
    let permitido = await isPermissionGranted();
    if (!permitido) permitido = (await requestPermission()) === "granted";
    if (permitido) sendNotification({ title: titulo, body: corpo });
  } catch (motivo) {
    // eslint-disable-next-line no-console
    console.error("[link seguro] falha ao notificar:", motivo);
  }
}

const hora = (ms: number) => new Date(ms).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });

/**
 * Pedidos de entrada pelo link seguro, em qualquer tela do app: o celular acertou o código do Authy e
 * espera o dono aprovar. Sem Permitir aqui, nenhum aparelho novo entra — é o que segura quem
 * conseguir adivinhar o código pela internet. Também avisa quando o pareamento foi pausado ou
 * desligado por excesso de códigos errados.
 */
export function AprovacaoDeAparelho() {
  const [lista, setLista] = useState<ListaDePedidos | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [ocupado, setOcupado] = useState(false);
  const avisados = useRef(new Set<string>());

  const avisarNovidades = useCallback((nova: ListaDePedidos) => {
    for (const pedido of nova.pedidos) {
      if (avisados.current.has(pedido.id)) continue;
      avisados.current.add(pedido.id);
      void notificar("Aparelho quer entrar no OMNI", `${descreverAparelho(pedido.aparelho)} (${pedido.ip}). Permita ou recuse no app.`);
    }
    const chave = nova.alerta ? `alerta:${nova.alerta.em_ms}` : null;
    if (chave && !avisados.current.has(chave)) {
      avisados.current.add(chave);
      void notificar("Link seguro do OMNI", nova.alerta!.texto);
    }
  }, []);

  useEffect(() => {
    let cancelado = false;
    let timer: number | undefined;
    const ler = async () => {
      let espera = INTERVALO_MS;
      try {
        const nova = await chamar("GET", "/tunel/pedidos");
        if (cancelado) return;
        setLista(nova);
        avisarNovidades(nova);
      } catch {
        espera = INTERVALO_COM_ERRO_MS;
      }
      if (!cancelado) timer = window.setTimeout(() => void ler(), espera);
    };
    void ler();
    return () => { cancelado = true; window.clearTimeout(timer); };
  }, [avisarNovidades]);

  async function acao(caminho: string) {
    setOcupado(true); setErro(null);
    try { setLista(await chamar("POST", caminho)); }
    catch (reason) { setErro(reason instanceof Error ? reason.message : String(reason)); }
    finally { setOcupado(false); }
  }

  const pedidos = lista?.pedidos ?? [];
  const alerta = lista?.alerta ?? null;
  const desligado = Boolean(lista?.pareamento_desligado);
  if (!pedidos.length && !alerta && !desligado && !erro) return null;

  return <div className="fixed bottom-4 right-4 z-50 flex w-[22rem] max-w-[calc(100vw-2rem)] flex-col gap-2" aria-live="polite">
    {pedidos.map((pedido) => <section key={pedido.id} role="alertdialog" aria-label="Aparelho quer entrar"
      className="glass space-y-2 rounded-none border-l-2 border-accent p-4 shadow-lg">
      <p className="flex items-center gap-2 text-xs font-medium text-text-primary">
        <ShieldIcon size={14} aria-hidden />Um aparelho quer entrar pelo link seguro
      </p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11px] text-text-secondary">
        <dt className="text-text-muted">Aparelho</dt><dd title={pedido.aparelho}>{descreverAparelho(pedido.aparelho)}</dd>
        <dt className="text-text-muted">IP</dt><dd><code>{pedido.ip}</code></dd>
        <dt className="text-text-muted">Hora</dt><dd>{hora(pedido.em_ms)}</dd>
      </dl>
      <p className="text-[10px] text-text-muted">Só permita se foi você, agora. O pedido some às {hora(pedido.expira_em_ms)}.</p>
      <div className="flex gap-2">
        <button type="button" className="remoto-acao is-forte" disabled={ocupado}
          onClick={() => void acao(`/tunel/pedidos/${encodeURIComponent(pedido.id)}/permitir`)}>
          <CheckIcon size={14} aria-hidden />Permitir
        </button>
        <button type="button" className="remoto-acao" disabled={ocupado}
          onClick={() => void acao(`/tunel/pedidos/${encodeURIComponent(pedido.id)}/recusar`)}>
          <CloseIcon size={14} aria-hidden />Recusar
        </button>
      </div>
    </section>)}

    {(alerta || desligado) && <section role="alert" className="glass space-y-2 rounded-none border-l-2 border-warning p-4 shadow-lg">
      <p className="flex items-start gap-2 text-xs text-warning">
        <WarningIcon size={14} aria-hidden className="mt-0.5 shrink-0" />
        {alerta?.texto ?? "O pareamento de aparelhos novos pelo link seguro está desligado."}
      </p>
      {alerta && lista?.pausado_ate_ms && <p className="text-[10px] text-text-muted">Volta sozinho às {hora(lista.pausado_ate_ms)}.</p>}
      <p className="text-[10px] text-text-muted">Se não foi você tentando, alguém tem o endereço do seu link. Desligue e ligue o link seguro para trocar de endereço.</p>
      <div className="flex gap-2">
        {desligado && <button type="button" className="remoto-acao is-forte" disabled={ocupado} onClick={() => void acao("/tunel/pareamento/reativar")}>
          <ReloadIcon size={14} aria-hidden />Reativar pareamento
        </button>}
        {alerta && !desligado && <button type="button" className="remoto-acao" disabled={ocupado} onClick={() => void acao("/tunel/alerta/visto")}>
          <CheckIcon size={14} aria-hidden />Entendi
        </button>}
      </div>
    </section>}

    {erro && <p role="alert" className="glass rounded-none border-l-2 border-danger p-3 text-xs text-danger">{erro}</p>}
  </div>;
}

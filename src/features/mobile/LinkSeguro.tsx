import { useCallback, useEffect, useState } from "react";
import { engineApi } from "../terminal/terminalService";
import { CheckIcon, CloudIcon, CopyIcon, PackageIcon, PlayIcon, ShieldIcon, StopIcon } from "../../components/ui/PixelIcon";
import { Acao, QrPersonalizado } from "./remotoUi";

/** O que o engine conta do link seguro (`GET /tunel`, só pelo desktop). */
export interface EstadoDoTunel {
  ligado: boolean;
  url: string | null;
  /** Endereço para o QR. Não leva token: o celular entra com o código do Authy. */
  qr: string | null;
  erro: string | null;
  totp_confirmed: boolean;
  cloudflared: {
    instalado: boolean;
    caminho: string | null;
    pode_baixar: boolean;
    instalando: boolean;
    progresso: number | null;
    erro: string | null;
    comando: string;
    versao: string;
  };
}

async function tunel(metodo: "GET" | "POST", caminho: string): Promise<EstadoDoTunel> {
  const resposta = await engineApi(metodo, caminho, metodo === "POST" ? "{}" : null,
    metodo === "POST" ? { headers: [["Content-Type", "application/json"]] } : {});
  let corpo: unknown = null;
  try { corpo = JSON.parse(resposta.body); } catch { /* corpo não-JSON vira a mensagem genérica abaixo */ }
  if (resposta.status >= 400 || !corpo) {
    const mensagem = (corpo as { error?: string } | null)?.error;
    throw new Error(mensagem || "O engine não respondeu sobre o link seguro.");
  }
  return corpo as EstadoDoTunel;
}

/** Ainda há algo a caminho: o download, ou o túnel ligado sem endereço. */
function aguardando(estado: EstadoDoTunel | null) {
  return Boolean(estado && (estado.cloudflared.instalando || (estado.ligado && !estado.url)));
}

/**
 * O caminho recomendado para o celular: o PC abre um link https pela Cloudflare (sem conta, grátis), o
 * celular lê o QR e digita o código do Authy uma vez. Sem instalar nada no celular, e com HTTPS de
 * verdade — é o que libera "instalar como app" e o microfone.
 */
export function LinkSeguro({ totpConfirmado, corQr, onConfigurarAuthy }: {
  totpConfirmado: boolean; corQr: string; onConfigurarAuthy: () => void;
}) {
  const [estado, setEstado] = useState<EstadoDoTunel | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [falhouLer, setFalhouLer] = useState(false);
  const [ocupado, setOcupado] = useState(false);
  const [copiado, setCopiado] = useState<"link" | "comando" | null>(null);

  const ler = useCallback(async () => {
    try { setEstado(await tunel("GET", "/tunel")); setFalhouLer(false); }
    catch { setFalhouLer(true); }
  }, []);

  // O Authy é confirmado na outra parte desta tela: relê para liberar o "Ligar" na hora.
  useEffect(() => { void ler(); }, [ler, totpConfirmado]);

  const esperando = aguardando(estado);
  useEffect(() => {
    if (!esperando) return;
    const timer = window.setInterval(() => void ler(), 1500);
    return () => window.clearInterval(timer);
  }, [esperando, ler]);

  async function acao(caminho: string) {
    setOcupado(true); setErro(null);
    try { setEstado(await tunel("POST", caminho)); }
    catch (reason) { setErro(reason instanceof Error ? reason.message : String(reason)); }
    finally { setOcupado(false); }
  }

  function copiar(texto: string, qual: "link" | "comando") {
    void navigator.clipboard?.writeText(texto).then(() => setCopiado(qual)).catch(() => setErro("Não consegui copiar; selecione e copie à mão."));
  }

  const authyOk = estado ? estado.totp_confirmed : totpConfirmado;
  const cf = estado?.cloudflared;
  const pronto = Boolean(estado?.ligado && estado.url && estado.qr);

  return <section className="glass remoto-passo-corpo space-y-4 rounded-none p-5" aria-label="Link seguro">
    <div className="flex flex-wrap items-center gap-3">
      <span aria-hidden className="remoto-icone"><CloudIcon size={18} /></span>
      <h3 className="pixel-text min-w-0 flex-1 text-sm text-text-primary">Pelo link seguro (sem instalar nada no celular)</h3>
      <span className="bg-accent/20 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-accent">recomendado</span>
      {pronto && <span className="remoto-chip is-ok">no ar</span>}
    </div>
    <p className="text-xs text-text-secondary">
      O PC cria um endereço <strong>https</strong> só seu. No celular, você lê o QR, digita o código do
      Authy uma vez e pronto: funciona no 4G, dá para instalar como app e usar o microfone.
    </p>

    {falhouLer && !estado && <p className="text-xs text-text-muted">Não consegui ler o estado do link seguro. Feche e abra esta aba de novo.</p>}

    {estado && !authyOk && <div className="space-y-2 border border-warning/60 bg-warning/5 p-3">
      <p className="text-xs text-warning">
        Antes, proteja com o Authy. O link fica na internet, então todo aparelho novo precisa do código de
        6 dígitos para entrar — mesmo quem tiver o QR.
      </p>
      <Acao forte icone={ShieldIcon} onClick={onConfigurarAuthy}>Proteger com o Authy primeiro</Acao>
    </div>}

    {estado && authyOk && cf && !cf.instalado && <div className="space-y-2">
      <p className="text-xs text-text-secondary">Falta o <strong>cloudflared</strong>, o programa da Cloudflare que abre o link neste PC.</p>
      {cf.instalando ? <div className="space-y-1">
        <p className="text-xs text-text-secondary">Baixando o cloudflared{cf.progresso != null ? `… ${cf.progresso}%` : "…"}</p>
        <div role="progressbar" aria-label="Baixando o cloudflared" aria-valuemin={0} aria-valuemax={100}
          aria-valuenow={cf.progresso ?? undefined} className="h-1.5 w-full bg-overlay/15">
          <div className="h-full bg-accent transition-[width]" ref={barra => { if (barra) barra.style.width = `${cf.progresso ?? 5}%`; }} />
        </div>
      </div> : cf.pode_baixar && <Acao forte disabled={ocupado} icone={PackageIcon} onClick={() => void acao("/tunel/instalar")}>Instalar o cloudflared</Acao>}
      <div className="flex flex-wrap items-center gap-2 text-xs text-text-muted">
        <span>{cf.pode_baixar ? "Ou, no terminal:" : "No terminal:"}</span>
        <code className="text-accent">{cf.comando}</code>
        <Acao icone={copiado === "comando" ? CheckIcon : CopyIcon} onClick={() => copiar(cf.comando, "comando")}>
          {copiado === "comando" ? "Comando copiado" : "Copiar comando"}
        </Acao>
      </div>
      {cf.instalado === false && !cf.instalando && cf.pode_baixar && <p className="text-[10px] text-text-muted">Depois de instalar pelo terminal, volte aqui: o botão Ligar aparece sozinho.</p>}
      {cf.erro && <p role="alert" className="border-l-2 border-danger pl-3 text-xs text-danger">{cf.erro}</p>}
    </div>}

    {estado && authyOk && cf?.instalado && !estado.ligado && <Acao forte disabled={ocupado} icone={PlayIcon} onClick={() => void acao("/tunel/ligar")}>Ligar</Acao>}

    {estado?.ligado && !pronto && <p role="status" className="text-xs text-warning">Ligando o link seguro… leva alguns segundos.</p>}

    {pronto && <div className="remoto-aparelho flex flex-wrap items-start gap-5">
      <QrPersonalizado texto={estado!.qr!} cor={corQr} alt="Código QR do link seguro" tamanho={240} />
      <div className="min-w-0 flex-1 space-y-3">
        <ol className="list-decimal space-y-1 pl-4 text-xs text-text-secondary">
          <li>Aponte a câmera do celular para o código.</li>
          <li>Digite o código de 6 dígitos do Authy quando pedir (só na primeira vez).</li>
          <li>Para virar app: toque em <strong>Instalar no celular</strong> lá dentro.</li>
        </ol>
        <div className="flex flex-wrap items-center gap-2">
          <code className="remoto-link min-w-0 flex-1 break-all">{estado!.url}</code>
          <Acao icone={copiado === "link" ? CheckIcon : CopyIcon} onClick={() => copiar(estado!.qr!, "link")}>
            {copiado === "link" ? "Link copiado" : "Copiar link"}
          </Acao>
        </div>
        <p className="text-[10px] text-text-muted">O endereço muda quando o PC reinicia; leia o QR de novo.</p>
      </div>
    </div>}

    {estado?.erro && <p role="alert" className="border-l-2 border-warning pl-3 text-xs text-warning">{estado.erro}</p>}
    {estado?.ligado && <Acao disabled={ocupado} icone={StopIcon} onClick={() => void acao("/tunel/desligar")}>Desligar</Acao>}
    {erro && <p role="alert" className="border-l-2 border-danger pl-3 text-xs text-danger">{erro}</p>}
  </section>;
}

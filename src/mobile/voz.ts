import { useEffect, useState } from "react";
import { request } from "./api";

/** O que o engine diz da voz (`GET /voz`). Sem `instalado`, o microfone não aparece em lugar nenhum. */
export interface EstadoDaVoz {
  instalado: boolean;
  baixando: boolean;
  baixado: number;
  total: number;
  erro: string | null;
  corrigir: boolean;
}

/** Microfone só existe em página segura (HTTPS ou localhost) — o `http://100.x` do Tailscale não é. */
export function navegadorGrava(): boolean {
  return typeof window !== "undefined" && window.isSecureContext
    && Boolean(navigator.mediaDevices?.getUserMedia) && typeof AudioContext !== "undefined";
}

interface Reconhecedor {
  lang: string; continuous: boolean; interimResults: boolean;
  onresult: ((evento: { results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }> }) => void) | null;
  onerror: ((evento: { error: string }) => void) | null;
  onend: (() => void) | null;
  start: () => void; stop: () => void; abort: () => void;
}

/**
 * Fala → texto do próprio navegador (Chrome/Edge/Safari mandam o áudio ao serviço deles, de graça,
 * sem modelo no PC). Fora do app desktop: o WebView2 tem o objeto mas não o serviço, e só dá erro.
 */
export function falaDoNavegador(): (new () => Reconhecedor) | null {
  if (typeof window === "undefined" || "__TAURI_INTERNALS__" in window || !window.isSecureContext) return null;
  const w = window as unknown as Record<string, unknown>;
  return (w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null) as (new () => Reconhecedor) | null;
}

export interface Escuta { parar: () => void; cancelar: () => void }

/** Erro `network` da fala do navegador: ele não alcança o serviço de fala. Brave, Opera, Vivaldi e o
 *  Chromium puro sempre dão isso (não têm o serviço do Google); Chrome e Edge, só sem internet. */
export const FALA_SEM_SERVICO = "Este navegador não tem o serviço de fala (Brave, Opera e Chromium não têm). Baixe o modelo de voz em Configurações > Voz, no OMNI do PC, para usar a voz do próprio PC; ou use o Chrome ou o Edge.";

const LIBERAR_NO_NAVEGADOR = "O navegador bloqueou o microfone. Clique no cadeado ao lado do endereço, libere o microfone para esta página e tente de novo.";
const LIBERAR_NO_SISTEMA = "O sistema bloqueou o microfone. Confira nas configurações de privacidade do computador se o OMNI pode usar o microfone e tente de novo.";
const SEM_MICROFONE = "Nenhum microfone encontrado. Conecte um microfone e tente de novo.";
const MICROFONE_OCUPADO = "O microfone está em uso por outro programa. Feche o outro programa (chamada, gravador) e tente de novo.";
export const SO_EM_HTTPS = "O navegador só libera o microfone em endereço https. Use o link https do Acesso remoto.";

function noApp(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
function paginaSegura(): boolean {
  return typeof window !== "undefined" && window.isSecureContext;
}

/** Frase para a pessoa quando o microfone não liga (`getUserMedia` falhou). Cada motivo pede uma ação
 *  diferente: liberar a permissão, conectar um microfone, fechar outro programa ou trocar para https. */
export function motivoDoMicrofone(erro: unknown, onde: { seguro: boolean; noApp: boolean } = { seguro: paginaSegura(), noApp: noApp() }): string {
  if (!onde.seguro) return SO_EM_HTTPS;
  const nome = erro instanceof Error || erro instanceof DOMException ? erro.name : "";
  if (nome === "NotAllowedError" || nome === "PermissionDeniedError" || nome === "SecurityError") return onde.noApp ? LIBERAR_NO_SISTEMA : LIBERAR_NO_NAVEGADOR;
  if (nome === "NotFoundError" || nome === "DevicesNotFoundError" || nome === "OverconstrainedError") return SEM_MICROFONE;
  if (nome === "NotReadableError" || nome === "TrackStartError" || nome === "AbortError") return MICROFONE_OCUPADO;
  return "Não deu para ligar o microfone. Tente de novo; se continuar, confira se ele está conectado.";
}

/** Frase para o erro da fala do navegador; `null` quando não é erro (silêncio ou o nosso cancelar). */
export function motivoDaFala(codigo: string): string | null {
  if (codigo === "not-allowed") return LIBERAR_NO_NAVEGADOR;
  if (codigo === "network" || codigo === "service-not-allowed") return FALA_SEM_SERVICO;
  if (codigo === "audio-capture") return `${SEM_MICROFONE} Se houver um, ele pode estar em uso por outro programa.`;
  return null;
}

/** Brave avisa de si em `navigator.brave`; ele tem o objeto de fala, mas não o serviço do Google. */
export function ehBrave(): boolean {
  return typeof navigator !== "undefined" && "brave" in navigator;
}

/** Grava pela voz do PC (whisper) em vez da fala do navegador: quando o navegador não tem o serviço
 *  (Brave sempre; outros depois do primeiro erro) e o modelo está baixado. */
export function preferirVozDoPc({ brave, semServico, vozDoPc }: { brave: boolean; semServico: boolean; vozDoPc: boolean }): boolean {
  return vozDoPc && (brave || semServico);
}
const CHAVE_SEM_SERVICO = "omni-fala-sem-servico";
/** Já deu `network` nesta aba: o microfone passa a usar a voz do PC, se houver. */
export function falaSemServico(): boolean {
  try { return sessionStorage.getItem(CHAVE_SEM_SERVICO) === "1"; } catch { return false; }
}
function marcarSemServico() {
  try { sessionStorage.setItem(CHAVE_SEM_SERVICO, "1"); } catch { /* sem armazenamento: tenta de novo na próxima */ }
}

/**
 * Escuta contínua: `aoTexto` recebe a fala inteira até agora (final + parcial) a cada palavra. O Chrome
 * encerra sozinho depois de um silêncio; aqui religa até a pessoa parar, guardando o que já veio.
 */
export function ouvir(aoTexto: (falado: string) => void, aoFim: (erro: string | null) => void): Escuta {
  const Classe = falaDoNavegador();
  if (!Classe) throw new Error("Este navegador não reconhece fala");
  let guardado = "";
  let atual = "";
  let ativo = true;
  let erro: string | null = null;
  const juntar = (a: string, b: string) => (a && b ? `${a} ${b}` : a || b);
  const iniciar = () => {
    const r = new Classe();
    r.lang = navigator.language || "pt-BR";
    r.continuous = true;
    r.interimResults = true;
    // ponytail: remonta tudo de `results` a cada evento; o Chrome do Android às vezes repete trechos
    // finais no modo contínuo. Se aparecer texto dobrado, deduplicar por resultado.
    r.onresult = (evento) => {
      atual = Array.from(evento.results, (res) => res[0]?.transcript.trim() ?? "").filter(Boolean).join(" ");
      aoTexto(juntar(guardado, atual));
    };
    r.onerror = (evento) => {
      // "no-speech" é só silêncio; "aborted" é o nosso cancelar.
      const motivo = motivoDaFala(evento.error);
      if (!motivo) return;
      erro = motivo; ativo = false;
      if (motivo === FALA_SEM_SERVICO) marcarSemServico();
    };
    r.onend = () => {
      guardado = juntar(guardado, atual);
      atual = "";
      if (ativo) iniciar(); else aoFim(erro);
    };
    r.start();
    escuta.atual = r;
  };
  const escuta = {
    atual: null as Reconhecedor | null,
    parar: () => { ativo = false; escuta.atual?.stop(); },
    cancelar: () => { ativo = false; if (escuta.atual) { escuta.atual.onend = null; escuta.atual.abort(); } },
  };
  iniciar();
  return escuta;
}

/** Se o modelo de voz está no PC. Engine antigo (sem a rota) ou erro de rede contam como "não". */
export function useVozInstalada(): boolean {
  const [instalada, setInstalada] = useState(false);
  useEffect(() => {
    let vivo = true;
    const ler = () => request<EstadoDaVoz>("/voz").then((e) => { if (vivo) setInstalada(e.instalado); }).catch(() => undefined);
    void ler();
    // Baixou o modelo nas Configurações com a conversa aberta: o microfone aparece ao voltar.
    window.addEventListener("focus", ler);
    return () => { vivo = false; window.removeEventListener("focus", ler); };
  }, []);
  return instalada;
}

const TAXA = 16_000;
// ponytail: limiar fixo de volume (RMS); com a supressão de ruído do navegador ligada basta. Ambiente
// barulhento demais nunca "pausa" — aí o corte de 20 s segura. Calibrar pelo ruído de fundo se precisar.
const LIMIAR = 0.01;
const PAUSA = 0.6 * TAXA;
const TRECHO_MIN = 2 * TAXA;
const TRECHO_MAX = 20 * TAXA;

/**
 * Corta a fala em trechos nas pausas, para transcrever enquanto a pessoa ainda fala. Silêncio puro
 * nunca vira trecho: o whisper "ouve" frases inteiras no silêncio ("Obrigado.").
 */
export function criarCortador(aoTrecho: (amostras: Float32Array) => void) {
  let partes: Float32Array[] = [];
  let total = 0;
  let silencio = 0;
  let falou = false;
  const soltar = () => {
    if (falou) {
      const trecho = new Float32Array(total);
      let pos = 0;
      for (const parte of partes) { trecho.set(parte, pos); pos += parte.length; }
      aoTrecho(trecho);
    }
    partes = []; total = 0; silencio = 0; falou = false;
  };
  return {
    empurrar(bloco: Float32Array) {
      let soma = 0;
      for (const v of bloco) soma += v * v;
      const alto = Math.sqrt(soma / bloco.length) >= LIMIAR;
      // Antes da fala, guarda só o último bloco: o começo da primeira sílaba não se perde.
      if (!falou && !alto) { partes = [bloco]; total = bloco.length; return; }
      partes.push(bloco); total += bloco.length;
      if (alto) { falou = true; silencio = 0; } else silencio += bloco.length;
      if ((silencio >= PAUSA && total >= TRECHO_MIN) || total >= TRECHO_MAX) soltar();
    },
    fim: soltar,
  };
}

/** Média por janela: 48 kHz → 16 kHz. Para voz basta, e funciona em todo navegador (o Firefox recusa
 *  ligar o microfone num AudioContext de 16 kHz). */
function reamostrar(entrada: Float32Array, taxa: number): Float32Array {
  const passo = taxa / TAXA;
  const saida = new Float32Array(Math.floor(entrada.length / passo));
  for (let i = 0; i < saida.length; i += 1) {
    const ini = Math.floor(i * passo), fim = Math.max(ini + 1, Math.floor((i + 1) * passo));
    let soma = 0;
    for (let k = ini; k < fim; k += 1) soma += entrada[k];
    saida[i] = soma / (fim - ini);
  }
  return saida;
}

/** PCM 16 bits mono, 16 kHz — o formato que o engine entrega ao whisper. */
export function paraPcm16(amostras: Float32Array): Blob {
  const pcm = new Int16Array(amostras.length);
  for (let i = 0; i < amostras.length; i += 1) {
    const a = Math.max(-1, Math.min(1, amostras[i]));
    pcm[i] = a < 0 ? a * 0x8000 : a * 0x7fff;
  }
  return new Blob([pcm.buffer], { type: "application/octet-stream" });
}

export interface Gravacao {
  /** Para e solta o que restou como último trecho. */
  parar: () => void;
  cancelar: () => void;
}

export async function gravar(aoTrecho: (audio: Blob) => void): Promise<Gravacao> {
  // Endereço http: o navegador nem expõe `mediaDevices`.
  if (!navigator.mediaDevices?.getUserMedia) throw new TypeError("microfone indisponível nesta página");
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
  const contexto = new AudioContext();
  const cortador = criarCortador((amostras) => aoTrecho(paraPcm16(amostras)));
  // ponytail: ScriptProcessor está obsoleto mas roda em todo navegador sem arquivo extra; trocar por
  // AudioWorklet quando algum navegador tirar.
  const processador = contexto.createScriptProcessor(4096, 1, 1);
  processador.onaudioprocess = (evento) => cortador.empurrar(reamostrar(evento.inputBuffer.getChannelData(0), contexto.sampleRate));
  const fonte = contexto.createMediaStreamSource(stream);
  fonte.connect(processador);
  processador.connect(contexto.destination);
  const soltar = () => {
    processador.onaudioprocess = null;
    fonte.disconnect(); processador.disconnect();
    stream.getTracks().forEach((faixa) => faixa.stop());
    void contexto.close();
  };
  return {
    parar: () => { soltar(); cortador.fim(); },
    cancelar: soltar,
  };
}

export async function transcrever(conversa: string, audio: Blob): Promise<string> {
  const resposta = await request<{ texto: string }>(`/conversas/${encodeURIComponent(conversa)}/voz`, {
    method: "POST",
    body: audio,
    headers: { "Content-Type": "application/octet-stream" },
    // Trecho de até 20 s no CPU, com outros na fila: passa fácil dos 15 s padrão.
    signal: AbortSignal.timeout(120_000),
  });
  return resposta.texto;
}

/** Correção pelo Claude sobre a fala inteira; o engine devolve o mesmo texto se ela estiver desligada. */
export async function corrigir(conversa: string, texto: string): Promise<string> {
  const resposta = await request<{ texto: string }>(`/conversas/${encodeURIComponent(conversa)}/voz/corrigir`, {
    method: "POST",
    body: JSON.stringify({ texto }),
    headers: { "Content-Type": "application/json" },
    signal: AbortSignal.timeout(60_000),
  });
  return resposta.texto;
}

/** Palavra que a voz erra: o certo e as formas como ela já foi ouvida errado (`voz.rs`). */
export interface Palavra { termo: string; ouvido: string[]; origem?: string }
export interface Vocabulario { itens: Palavra[]; projetos: { nome: string; termos: string[] }[] }

const JSON_POST = { method: "POST", headers: { "Content-Type": "application/json" } } as const;
export const lerVocabulario = () => request<Vocabulario>("/voz/vocabulario");
export const salvarVocabulario = (itens: Palavra[]) =>
  request<Vocabulario>("/voz/vocabulario", { ...JSON_POST, body: JSON.stringify({ itens }) });
export const aprenderPalavras = (itens: Palavra[]) =>
  request<Vocabulario>("/voz/vocabulario/aprender", { ...JSON_POST, body: JSON.stringify({ itens }) });

/** Troca cada forma ouvida pelo termo certo. Sem diferenciar caixa, e hífen vale espaço: o whisper
 *  escreve "abra-se e a plus" numa vez e "abra se e a plus" na outra. */
export function aplicarVocabulario(texto: string, itens: Palavra[]): string {
  let saida = texto;
  for (const { termo, ouvido } of itens) {
    for (const forma of ouvido) {
      const partes = forma.trim().split(/[\s-]+/).filter(Boolean).map((p) => p.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
      if (partes.length === 0) continue;
      saida = saida.replace(new RegExp(`(?<![\\p{L}\\p{N}])${partes.join("[\\s-]+")}(?![\\p{L}\\p{N}])`, "giu"), termo);
    }
  }
  return saida;
}

/** Trechos que a pessoa trocou no texto ditado ("abra-se e a plus" → "SIAPLUS"): diff por palavra.
 *  Só trocas curtas viram sugestão — reescrever uma frase inteira não é palavra mal ouvida. */
export function substituicoes(ditado: string, final: string): Palavra[] {
  const a = ditado.split(/\s+/).filter(Boolean), b = final.split(/\s+/).filter(Boolean);
  if (a.length > 400 || b.length > 400) return [];
  // Caixa conta para alinhar ("domus" → "Domus" faz parte da correção); só a pontuação das pontas não.
  const norm = (p: string) => p.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
  const limpa = (ps: string[]) => ps.join(" ").replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
  // LCS de trás para frente: lcs[i][j] = maior trecho comum de a[i..] e b[j..].
  const lcs = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) for (let j = b.length - 1; j >= 0; j--)
    lcs[i][j] = norm(a[i]) === norm(b[j]) ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const trocas: Palavra[] = [];
  let i = 0, j = 0, saiu: string[] = [], entrou: string[] = [];
  const fecha = () => {
    const ouvido = limpa(saiu), termo = limpa(entrou);
    if (saiu.length && entrou.length && saiu.length <= 6 && entrou.length <= 4 && termo.length >= 2
      && ouvido.replace(/[\s-]+/g, "").toLowerCase() !== termo.replace(/[\s-]+/g, "").toLowerCase()
      && !trocas.some((t) => t.termo === termo && t.ouvido[0] === ouvido)) trocas.push({ termo, ouvido: [ouvido] });
    saiu = []; entrou = [];
  };
  // Igual só vira par se pular não custar nada: "abra-se e a plus e ver" contra "SIAPLUS e ver"
  // casa o segundo "e", e a troca inteira fica junta em vez de partida no primeiro.
  while (i < a.length || j < b.length) {
    const atual = lcs[i][j];
    if (i < a.length && lcs[i + 1][j] === atual) saiu.push(a[i++]);
    else if (j < b.length && lcs[i][j + 1] === atual) entrou.push(b[j++]);
    else { fecha(); i++; j++; } // pular qualquer um perderia: as duas palavras são iguais
  }
  fecha();
  return trocas;
}

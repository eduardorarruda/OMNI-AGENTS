import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY, type Simulation, type SimulationLinkDatum, type SimulationNodeDatum } from "d3-force";
import { grausDasNotas, vizinhas, type Grafo } from "./grafoModel";
import { corDoProjeto, enquadrar, escolherRotulos, raioDoNo, ZOOM_MAX, ZOOM_MIN, type Camera, type Tema } from "./grafoDesenho";

interface No extends SimulationNodeDatum {
  slug: string;
  titulo: string;
  projeto: string;
  raio: number;
  grau: number;
}
type Elo = SimulationLinkDatum<No> & { peso: number };

export interface GrafoCanvasHandle {
  /** Enquadra o grafo inteiro. */
  centralizar(): void;
  /** Leva a câmera até a nota (com zoom de leitura). */
  focar(slug: string): void;
  /** Zoom pelo centro da tela: fator > 1 aproxima. */
  aproximar(fator: number): void;
}

interface GrafoCanvasProps {
  grafo: Grafo;
  selecionada: string | null;
  /** Resultado da busca: ganha anel e rótulo. */
  destacadas?: ReadonlySet<string>;
  /** Índice de cor por projeto, calculado sobre o grafo inteiro (filtrar não troca cores). */
  paleta?: ReadonlyMap<string, number>;
  aoSelecionar: (slug: string | null) => void;
}

interface Cores {
  tema: Tema;
  fundo: string;
  texto: string;
  linha: string;
  semProjeto: string;
  destaque: string;
}

const TOQUE_EXTRA = 4;
const ARRASTE_MINIMO = 4;
const ZOOM_DE_LEITURA = 1.4;
const DURACAO_DA_CAMERA_MS = 320;
const DURACAO_DO_FOCO_MS = 160;
const FONTE = "500 11px ui-sans-serif, system-ui, sans-serif";
const ALTURA_DO_ROTULO = 14;
const TITULO_MAX = 32;
const ESMAECIDO_NO = 0.22;
const ESMAECIDO_ELO = 0.06;
/** Afastado, o nó não encolhe abaixo desta fração do tamanho nominal (senão vira poeira). */
const ESCALA_MINIMA_DO_NO = 0.55;

/** Raio em unidades do mundo que respeita o piso de tamanho na tela. */
const raioVisto = (n: { raio: number }, k: number) => n.raio * Math.max(1, ESCALA_MINIMA_DO_NO / k);

function lerCores(el: Element): Cores {
  const estilo = getComputedStyle(el);
  const rgb = (nome: string, padrao: string) => `rgb(${estilo.getPropertyValue(nome).trim().split(/\s+/).join(",") || padrao})`;
  const tema: Tema = document.documentElement.dataset.theme === "claro" ? "claro" : "escuro";
  return {
    tema,
    fundo: rgb("--c-bg-primary", tema === "claro" ? "249,249,251" : "14,14,20"),
    texto: rgb("--c-text-primary", tema === "claro" ? "24,24,32" : "226,226,236"),
    linha: rgb("--c-text-secondary", tema === "claro" ? "82,82,104" : "136,136,168"),
    semProjeto: rgb("--c-text-secondary", tema === "claro" ? "82,82,104" : "136,136,168"),
    destaque: rgb("--c-accent", "249,115,22"),
  };
}

/** O buffer do canvas segue o tamanho na tela vezes a densidade de pixels; o desenho e o clique usam px CSS. */
function ajustarTamanho(el: HTMLCanvasElement): { largura: number; altura: number } {
  const largura = el.clientWidth || 600, altura = el.clientHeight || 400;
  const dpr = window.devicePixelRatio || 1;
  el.width = Math.round(largura * dpr);
  el.height = Math.round(altura * dpr);
  return { largura, altura };
}

const suave = (t: number) => 1 - (1 - t) ** 3;
const encurtar = (titulo: string) => (titulo.length > TITULO_MAX ? `${titulo.slice(0, TITULO_MAX - 1)}…` : titulo);

/** Grafo em canvas: zoom na roda, arrastar o fundo (pan) e os nós, hover destaca a vizinhança.
 *  Só redesenha quando algo muda (simulação andando, câmera, foco); parado, não gasta nada.
 *  A lista de notas ao lado é o caminho acessível: o canvas sozinho não é navegável por teclado. */
export const GrafoCanvas = forwardRef<GrafoCanvasHandle, GrafoCanvasProps>(function GrafoCanvas(
  { grafo, selecionada, destacadas, paleta, aoSelecionar }, ref,
) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const nos = useRef<No[]>([]);
  const porSlug = useRef(new Map<string, No>());
  const elos = useRef<Elo[]>([]);
  const simulacao = useRef<Simulation<No, Elo> | null>(null);
  const visivel = useRef(true);
  const tamanho = useRef({ largura: 600, altura: 400 });
  const camera = useRef<Camera>({ x: 300, y: 200, k: 1 });
  const animacao = useRef<{ de: Camera; para: Camera; inicio: number } | null>(null);
  /** Até o usuário mexer na câmera, ela acompanha o layout e enquadra tudo. */
  const seguirLayout = useRef(true);
  const cores = useRef<Cores | null>(null);
  const larguras = useRef(new Map<string, number>());
  const hover = useRef<string | null>(null);
  const foco = useRef({ slug: null as string | null, mistura: 0, desde: 0, alvo: 0 });
  const quadro = useRef<number | null>(null);
  const arraste = useRef<{ tipo: "no" | "fundo"; no?: No; x0: number; y0: number; moveu: boolean } | null>(null);
  const props = useRef({ grafo, selecionada, destacadas, paleta, aoSelecionar });
  props.current = { grafo, selecionada, destacadas, paleta, aoSelecionar };
  const desenhar = useRef<(agora: number) => void>(() => {});

  const pedirDesenho = () => {
    if (quadro.current !== null || typeof requestAnimationFrame === "undefined") return;
    quadro.current = requestAnimationFrame((agora) => { quadro.current = null; desenhar.current(agora); });
  };

  const pontos = () => nos.current.map((n) => ({ x: n.x ?? 0, y: n.y ?? 0, r: n.raio }));
  const moverCamera = (para: Camera, animar = true) => {
    if (animar) animacao.current = { de: { ...camera.current }, para, inicio: performance.now() };
    else { animacao.current = null; camera.current = para; }
    pedirDesenho();
  };

  /** Atualiza o alvo do esmaecimento: hover vence a seleção; trocar de nó não pisca. */
  const atualizarFoco = () => {
    const alvo = hover.current ?? props.current.selecionada;
    const f = foco.current;
    const agora = performance.now();
    if (alvo) {
      if (f.alvo !== 1) { f.desde = agora; f.alvo = 1; }
      f.slug = alvo;
    } else if (f.alvo !== 0) { f.desde = agora; f.alvo = 0; }
    pedirDesenho();
  };

  desenhar.current = (agora) => {
    const el = canvas.current;
    const ctx = el?.getContext("2d");
    if (!el || !ctx) return;
    const c = (cores.current ??= lerCores(el));
    const { grafo: g, selecionada: sel, destacadas: achadas, paleta: paletaDeCores } = props.current;
    const { largura, altura } = tamanho.current;
    let continuar = false;

    const anim = animacao.current;
    if (anim) {
      const t = Math.min(1, (agora - anim.inicio) / DURACAO_DA_CAMERA_MS), e = suave(t);
      camera.current = { x: anim.de.x + (anim.para.x - anim.de.x) * e, y: anim.de.y + (anim.para.y - anim.de.y) * e, k: anim.de.k + (anim.para.k - anim.de.k) * e };
      if (t < 1) continuar = true; else animacao.current = null;
    } else if (seguirLayout.current && simulacao.current && simulacao.current.alpha() > simulacao.current.alphaMin()) {
      camera.current = enquadrar(pontos(), largura, altura);
    }

    const f = foco.current;
    const passo = Math.min(1, (agora - f.desde) / DURACAO_DO_FOCO_MS);
    f.mistura = f.alvo === 1 ? Math.max(f.mistura, suave(passo)) : Math.min(f.mistura, 1 - suave(passo));
    if (f.mistura !== f.alvo) continuar = true;
    if (f.alvo === 0 && f.mistura === 0) f.slug = null;
    const focado = f.slug;
    const vizinhasDoFoco = focado ? vizinhas(g, focado) : new Set<string>();
    const noFoco = (slug: string) => slug === focado || vizinhasDoFoco.has(slug);
    const alfaNo = (slug: string) => (focado && !noFoco(slug) ? 1 - (1 - ESMAECIDO_NO) * f.mistura : 1);

    const { x: cx, y: cy, k } = camera.current;
    const dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, largura, altura);
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * cx, dpr * cy);

    // Ligações: um traço por faixa de peso; as do foco por cima, na cor de destaque.
    const larguraDaLinha = Math.max(0.6, 1 / Math.sqrt(k));
    const faixas: Elo[][] = [[], [], []];
    const doFoco: Elo[] = [];
    for (const l of elos.current) {
      const a = l.source as No, b = l.target as No;
      if (focado && (a.slug === focado || b.slug === focado)) doFoco.push(l);
      else faixas[l.peso >= 0.7 ? 2 : l.peso >= 0.35 ? 1 : 0].push(l);
    }
    const tracar = (lista: Elo[]) => {
      ctx.beginPath();
      for (const l of lista) {
        const a = l.source as No, b = l.target as No;
        ctx.moveTo(a.x ?? 0, a.y ?? 0);
        ctx.lineTo(b.x ?? 0, b.y ?? 0);
      }
      ctx.stroke();
    };
    ctx.strokeStyle = c.linha;
    faixas.forEach((lista, i) => {
      if (lista.length === 0) return;
      const base = 0.3 + i * 0.16;
      ctx.globalAlpha = focado ? base - (base - ESMAECIDO_ELO) * f.mistura : base;
      ctx.lineWidth = (larguraDaLinha * (1 + i * 0.5)) / k;
      tracar(lista);
    });
    if (doFoco.length) {
      ctx.globalAlpha = 0.35 + 0.5 * f.mistura;
      ctx.strokeStyle = c.destaque;
      ctx.lineWidth = (larguraDaLinha * 1.8) / k;
      tracar(doFoco);
    }

    // Nós: anel do fundo separa nós encostados; seleção e busca ganham anel na cor de destaque.
    for (const n of nos.current) {
      ctx.globalAlpha = alfaNo(n.slug);
      ctx.beginPath();
      const r = raioVisto(n, k);
      ctx.arc(n.x ?? 0, n.y ?? 0, r, 0, Math.PI * 2);
      ctx.fillStyle = n.projeto ? corDoProjeto(n.projeto, c.tema, paletaDeCores) : c.semProjeto;
      ctx.fill();
      ctx.lineWidth = 1.5 / k;
      ctx.strokeStyle = c.fundo;
      ctx.stroke();
      if (n.slug === sel || n.slug === hover.current || achadas?.has(n.slug)) {
        ctx.beginPath();
        ctx.arc(n.x ?? 0, n.y ?? 0, r + 3 / k, 0, Math.PI * 2);
        ctx.lineWidth = (n.slug === sel ? 2.5 : 1.5) / k;
        ctx.strokeStyle = c.destaque;
        ctx.stroke();
      }
    }

    // Rótulos em px de tela (nítidos em qualquer zoom), sem sobreposição, com halo do fundo.
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.font = FONTE;
    ctx.textBaseline = "middle";
    const candidatos = [];
    for (const n of nos.current) {
      const raioNaTela = raioVisto(n, k) * k;
      const forcado = n.slug === sel || n.slug === focado || achadas?.has(n.slug) || (focado !== null && vizinhasDoFoco.has(n.slug));
      if (!forcado && raioNaTela < 7 && k < 1.5) continue;
      const sx = (n.x ?? 0) * k + cx, sy = (n.y ?? 0) * k + cy;
      if (sx < -200 || sy < -20 || sx > largura + 20 || sy > altura + 20) continue;
      let w = larguras.current.get(n.slug);
      if (w === undefined) { w = ctx.measureText(encurtar(n.titulo)).width; larguras.current.set(n.slug, w); }
      candidatos.push({ slug: n.slug, x: sx - w / 2, y: sy + raioNaTela + 3, largura: w, altura: ALTURA_DO_ROTULO, prioridade: (forcado ? 1e6 : 0) + n.grau });
    }
    const comRotulo = escolherRotulos(candidatos);
    ctx.lineJoin = "round";
    ctx.lineWidth = 3;
    ctx.strokeStyle = c.fundo;
    ctx.fillStyle = c.texto;
    for (const cand of candidatos) {
      if (!comRotulo.has(cand.slug)) continue;
      const n = porSlug.current.get(cand.slug);
      if (!n) continue;
      ctx.globalAlpha = alfaNo(cand.slug);
      const texto = encurtar(n.titulo), ty = cand.y + ALTURA_DO_ROTULO / 2;
      ctx.strokeText(texto, cand.x, ty);
      ctx.fillText(texto, cand.x, ty);
    }
    ctx.globalAlpha = 1;
    if (continuar) pedirDesenho();
  };

  // Simulação: só reconstrói quando o grafo muda; posições antigas são mantidas.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    tamanho.current = ajustarTamanho(el);
    const graus = grausDasNotas(grafo);
    const antigos = new Map(nos.current.map((n) => [n.slug, n]));
    nos.current = grafo.notas.map((nota) => {
      const grau = graus.get(nota.slug) ?? 0;
      const velho = antigos.get(nota.slug);
      return Object.assign(velho ?? { slug: nota.slug }, { titulo: nota.titulo, projeto: nota.projeto, grau, raio: raioDoNo(grau) });
    });
    porSlug.current = new Map(nos.current.map((n) => [n.slug, n]));
    larguras.current.clear();
    const grande = nos.current.length > 800;
    elos.current = grafo.ligacoes.filter((l) => porSlug.current.has(l.de) && porSlug.current.has(l.para))
      .map((l) => ({ source: l.de, target: l.para, peso: l.peso }));
    simulacao.current?.stop();
    const sim = forceSimulation<No>(nos.current)
      .alphaDecay(grande ? 0.045 : 0.03)
      .force("link", forceLink<No, Elo>(elos.current).id((n) => n.slug).distance((e) => 50 + (1 - e.peso) * 50).strength((e) => 0.08 + e.peso * 0.35))
      .force("carga", forceManyBody<No>().strength((n) => -120 - n.raio * 12).theta(grande ? 1.1 : 0.9).distanceMax(grande ? 350 : 700))
      .force("colisao", forceCollide<No>((n) => n.raio + 6).iterations(grande ? 1 : 2))
      .force("x", forceX<No>(0).strength(0.04))
      .force("y", forceY<No>(0).strength(0.04))
      .on("tick", pedirDesenho)
      .on("end", () => { if (seguirLayout.current) moverCamera(enquadrar(pontos(), tamanho.current.largura, tamanho.current.altura)); });
    if (!visivel.current) sim.stop();
    simulacao.current = sim;
    pedirDesenho();
    return () => { sim.stop(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [grafo]);

  useEffect(() => { atualizarFoco(); }, [selecionada]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { pedirDesenho(); }, [destacadas, paleta]); // eslint-disable-line react-hooks/exhaustive-deps

  // Tamanho, tema, visibilidade e roda: assinados uma vez.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const aoRedimensionar = () => {
      tamanho.current = ajustarTamanho(el);
      if (seguirLayout.current) moverCamera(enquadrar(pontos(), tamanho.current.largura, tamanho.current.altura), false);
      else pedirDesenho();
    };
    const redim = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(aoRedimensionar);
    redim?.observe(el);

    const tema = typeof MutationObserver === "undefined" ? null : new MutationObserver(() => { cores.current = null; pedirDesenho(); });
    tema?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-palette", "data-glass"] });

    const mudou = (aparece: boolean) => {
      visivel.current = aparece;
      const sim = simulacao.current;
      if (!sim) return;
      if (aparece) { if (sim.alpha() > sim.alphaMin()) sim.restart(); pedirDesenho(); } else sim.stop();
    };
    const intersecao = typeof IntersectionObserver === "undefined" ? null : new IntersectionObserver((itens) => mudou(itens.some((i) => i.isIntersecting)));
    intersecao?.observe(el);
    const aoMudarAba = () => mudou(!document.hidden);
    document.addEventListener("visibilitychange", aoMudarAba);

    // Roda fora do React: precisa de passive:false para não rolar a página junto.
    const aoRolar = (evento: WheelEvent) => {
      evento.preventDefault();
      const caixa = el.getBoundingClientRect();
      zoomEm(evento.clientX - caixa.left, evento.clientY - caixa.top, Math.exp(-evento.deltaY * 0.0015));
    };
    el.addEventListener("wheel", aoRolar, { passive: false });

    return () => {
      redim?.disconnect(); tema?.disconnect(); intersecao?.disconnect();
      document.removeEventListener("visibilitychange", aoMudarAba);
      el.removeEventListener("wheel", aoRolar);
      if (quadro.current !== null) cancelAnimationFrame(quadro.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function zoomEm(sx: number, sy: number, fator: number) {
    seguirLayout.current = false;
    animacao.current = null;
    const { x, y, k } = camera.current;
    const novo = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, k * fator));
    const wx = (sx - x) / k, wy = (sy - y) / k;
    camera.current = { k: novo, x: sx - wx * novo, y: sy - wy * novo };
    pedirDesenho();
  }

  useImperativeHandle(ref, () => ({
    centralizar() {
      seguirLayout.current = true;
      moverCamera(enquadrar(pontos(), tamanho.current.largura, tamanho.current.altura));
    },
    focar(slug) {
      const n = porSlug.current.get(slug);
      if (!n) return;
      seguirLayout.current = false;
      const k = Math.max(camera.current.k, ZOOM_DE_LEITURA);
      moverCamera({ k, x: tamanho.current.largura / 2 - (n.x ?? 0) * k, y: tamanho.current.altura / 2 - (n.y ?? 0) * k });
    },
    aproximar(fator) {
      const caixa = { x: tamanho.current.largura / 2, y: tamanho.current.altura / 2 };
      const { x, y, k } = camera.current;
      const novo = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, k * fator));
      seguirLayout.current = false;
      moverCamera({ k: novo, x: caixa.x - ((caixa.x - x) / k) * novo, y: caixa.y - ((caixa.y - y) / k) * novo });
    },
  }));

  function noEm(sx: number, sy: number): No | null {
    const { x, y, k } = camera.current;
    const wx = (sx - x) / k, wy = (sy - y) / k;
    let melhor: No | null = null, menor = Infinity;
    for (const n of nos.current) {
      const d = Math.hypot((n.x ?? 0) - wx, (n.y ?? 0) - wy);
      if (d <= raioVisto(n, k) + TOQUE_EXTRA / k && d < menor) { melhor = n; menor = d; }
    }
    return melhor;
  }

  const posicao = (evento: React.PointerEvent<HTMLCanvasElement>) => {
    const caixa = evento.currentTarget.getBoundingClientRect();
    return { sx: evento.clientX - caixa.left, sy: evento.clientY - caixa.top };
  };

  function apertou(evento: React.PointerEvent<HTMLCanvasElement>) {
    if (evento.button !== 0) return;
    const { sx, sy } = posicao(evento);
    const no = noEm(sx, sy);
    evento.currentTarget.setPointerCapture?.(evento.pointerId);
    arraste.current = { tipo: no ? "no" : "fundo", no: no ?? undefined, x0: sx, y0: sy, moveu: false };
  }

  function moveu(evento: React.PointerEvent<HTMLCanvasElement>) {
    const { sx, sy } = posicao(evento);
    const a = arraste.current;
    const el = evento.currentTarget;
    if (!a) {
      const no = noEm(sx, sy);
      el.style.cursor = no ? "pointer" : "grab";
      if ((no?.slug ?? null) !== hover.current) { hover.current = no?.slug ?? null; atualizarFoco(); }
      return;
    }
    if (!a.moveu && Math.hypot(sx - a.x0, sy - a.y0) < ARRASTE_MINIMO) return;
    if (!a.moveu) {
      a.moveu = true;
      seguirLayout.current = false;
      animacao.current = null;
      if (a.tipo === "no") simulacao.current?.alphaTarget(0.25).restart();
    }
    el.style.cursor = "grabbing";
    const { x, y, k } = camera.current;
    if (a.tipo === "no" && a.no) {
      a.no.fx = (sx - x) / k;
      a.no.fy = (sy - y) / k;
    } else {
      camera.current = { k, x: x + (sx - a.x0), y: y + (sy - a.y0) };
      a.x0 = sx; a.y0 = sy;
    }
    pedirDesenho();
  }

  function soltou(evento: React.PointerEvent<HTMLCanvasElement>) {
    const a = arraste.current;
    arraste.current = null;
    if (!a) return;
    evento.currentTarget.style.cursor = a.tipo === "no" ? "pointer" : "grab";
    if (a.tipo === "no" && a.no) {
      a.no.fx = null; a.no.fy = null;
      if (a.moveu) simulacao.current?.alphaTarget(0);
    }
    if (a.moveu) return;
    const { aoSelecionar, selecionada: sel } = props.current;
    if (a.no) aoSelecionar(a.no.slug === sel ? null : a.no.slug);
    else aoSelecionar(null);
  }

  function saiu() {
    if (hover.current !== null && !arraste.current) { hover.current = null; atualizarFoco(); }
  }

  return (
    <canvas ref={canvas} aria-hidden="true" onPointerDown={apertou} onPointerMove={moveu} onPointerUp={soltou}
      onPointerCancel={soltou} onPointerLeave={saiu} className="absolute inset-0 block h-full w-full touch-none cursor-grab select-none" />
  );
});

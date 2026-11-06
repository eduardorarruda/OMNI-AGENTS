import { useEffect, useRef } from "react";
import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, type Simulation, type SimulationLinkDatum, type SimulationNodeDatum } from "d3-force";
import { matizDoProjeto, vizinhas, type Grafo } from "./grafoModel";

interface No extends SimulationNodeDatum {
  slug: string;
  projeto: string;
}
type Elo = SimulationLinkDatum<No> & { peso: number };

interface GrafoCanvasProps {
  grafo: Grafo;
  selecionada: string | null;
  aoSelecionar: (slug: string) => void;
}

const RAIO = 5;
const RAIO_DO_TOQUE = 12;

/** O buffer do canvas segue o tamanho na tela vezes a densidade de pixels; o desenho e o clique usam px CSS. */
function ajustarTamanho(el: HTMLCanvasElement): { largura: number; altura: number } {
  const largura = el.clientWidth || 600, altura = el.clientHeight || 400;
  const dpr = window.devicePixelRatio || 1;
  el.width = Math.round(largura * dpr);
  el.height = Math.round(altura * dpr);
  return { largura, altura };
}

/** Desenho em canvas; o layout de forças para quando a aba some da tela. A lista de notas ao lado é o
 *  caminho acessível: o canvas sozinho não é navegável por teclado. */
export function GrafoCanvas({ grafo, selecionada, aoSelecionar }: GrafoCanvasProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const nos = useRef<No[]>([]);
  const simulacao = useRef<Simulation<No, Elo> | null>(null);
  const visivel = useRef(true);
  const estado = useRef({ grafo, selecionada });
  estado.current = { grafo, selecionada };
  const desenhar = useRef<() => void>(() => {});

  desenhar.current = () => {
    const el = canvas.current;
    const ctx = el?.getContext("2d");
    if (!el || !ctx) return;
    const { grafo: g, selecionada: sel } = estado.current;
    const vizinhasDaSel = sel ? vizinhas(g, sel) : new Set<string>();
    const estilo = getComputedStyle(el);
    const rgb = (nome: string, padrao: string) => `rgb(${estilo.getPropertyValue(nome).trim().split(/\s+/).join(",") || padrao})`;
    const traco = rgb("--c-text-muted", "136,136,168");
    const destaque = rgb("--c-accent", "249,115,22");
    const porSlug = new Map(nos.current.map((n) => [n.slug, n]));
    const dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, el.clientWidth, el.clientHeight);
    for (const l of g.ligacoes) {
      const a = porSlug.get(l.de), b = porSlug.get(l.para);
      if (!a || !b) continue;
      const ativa = sel !== null && (l.de === sel || l.para === sel);
      ctx.globalAlpha = sel === null || ativa ? 0.35 + l.peso * 0.5 : 0.08;
      ctx.strokeStyle = ativa ? destaque : traco;
      ctx.lineWidth = 0.6 + l.peso * 1.6;
      ctx.beginPath();
      ctx.moveTo(a.x ?? 0, a.y ?? 0);
      ctx.lineTo(b.x ?? 0, b.y ?? 0);
      ctx.stroke();
    }
    for (const n of nos.current) {
      const apagada = sel !== null && n.slug !== sel && !vizinhasDaSel.has(n.slug);
      ctx.globalAlpha = apagada ? 0.25 : 1;
      ctx.fillStyle = n.projeto ? `hsl(${matizDoProjeto(n.projeto)} 65% 58%)` : traco;
      ctx.beginPath();
      ctx.arc(n.x ?? 0, n.y ?? 0, n.slug === sel ? RAIO + 2 : RAIO, 0, Math.PI * 2);
      ctx.fill();
      if (n.slug === sel) { ctx.lineWidth = 2; ctx.strokeStyle = destaque; ctx.stroke(); }
    }
    ctx.globalAlpha = 1;
  };

  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const { largura, altura } = ajustarTamanho(el);
    const antigos = new Map(nos.current.map((n) => [n.slug, n]));
    nos.current = grafo.notas.map((nota) => antigos.get(nota.slug) ?? { slug: nota.slug, projeto: nota.projeto });
    const elos: Elo[] = grafo.ligacoes.map((l) => ({ source: l.de, target: l.para, peso: l.peso }));
    simulacao.current?.stop();
    const sim = forceSimulation<No>(nos.current)
      .force("link", forceLink<No, Elo>(elos).id((n) => n.slug).distance(60).strength((e) => 0.1 + e.peso * 0.4))
      .force("carga", forceManyBody().strength(-70))
      .force("colisao", forceCollide(RAIO + 2))
      .force("centro", forceCenter(largura / 2, altura / 2))
      .on("tick", () => desenhar.current());
    if (!visivel.current) sim.stop();
    simulacao.current = sim;
    return () => { sim.stop(); };
  }, [grafo]);

  useEffect(() => { desenhar.current(); }, [selecionada]);

  useEffect(() => {
    const el = canvas.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observador = new ResizeObserver(() => {
      const { largura, altura } = ajustarTamanho(el);
      const sim = simulacao.current;
      if (sim) {
        sim.force("centro", forceCenter(largura / 2, altura / 2));
        if (visivel.current) sim.alpha(0.3).restart();
      }
      desenhar.current();
    });
    observador.observe(el);
    return () => observador.disconnect();
  }, []);

  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const mudou = (aparece: boolean) => {
      visivel.current = aparece;
      const sim = simulacao.current;
      if (!sim) return;
      if (aparece) { if (sim.alpha() > sim.alphaMin()) sim.restart(); else desenhar.current(); } else sim.stop();
    };
    const observador = typeof IntersectionObserver === "undefined" ? null : new IntersectionObserver((itens) => mudou(itens.some((i) => i.isIntersecting)));
    observador?.observe(el);
    const aoMudarAba = () => mudou(!document.hidden);
    document.addEventListener("visibilitychange", aoMudarAba);
    return () => { observador?.disconnect(); document.removeEventListener("visibilitychange", aoMudarAba); };
  }, []);

  function clicou(evento: React.MouseEvent<HTMLCanvasElement>) {
    const caixa = evento.currentTarget.getBoundingClientRect();
    const x = evento.clientX - caixa.left, y = evento.clientY - caixa.top;
    let melhor: No | null = null, menor = RAIO_DO_TOQUE;
    for (const n of nos.current) {
      const d = Math.hypot((n.x ?? 0) - x, (n.y ?? 0) - y);
      if (d <= menor) { melhor = n; menor = d; }
    }
    if (melhor) aoSelecionar(melhor.slug);
  }

  return <canvas ref={canvas} onClick={clicou} aria-hidden="true" className="h-full min-h-48 w-full cursor-pointer" />;
}

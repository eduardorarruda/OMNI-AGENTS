import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Input, Select, type SelectOption } from "../../components/ui";
import { CloseIcon, SearchIcon, ShapesIcon } from "../../components/ui/PixelIcon";
import { GrafoCanvas, type GrafoCanvasHandle } from "./GrafoCanvas";
import { corDoProjeto, paletaDosProjetos, type Tema } from "./grafoDesenho";
import { buscarNotas, contarPorTipo, filtrarGrafo, TIPOS_DE_LIGACAO, vizinhas, type Grafo, type TipoDeLigacao } from "./grafoModel";

interface GrafoPanelProps {
  /** Presente só no painel da lateral: abre o mesmo grafo numa aba grande. */
  onAbrirGrande?: () => void;
}

const TODOS_OS_TIPOS = TIPOS_DE_LIGACAO.map((t) => t.tipo);
const TODOS = "";
const MAX_NA_LEGENDA = 6;
const BUSCA_COM_LISTA = 12;

const plural = (n: number, um: string, varios: string) => `${n.toLocaleString("pt-BR")} ${n === 1 ? um : varios}`;

/** Tema atual (claro/escuro) acompanhando a troca nas Configurações, para as bolinhas de cor da legenda. */
function useTema(): Tema {
  return useSyncExternalStore(
    (avisar) => {
      if (typeof MutationObserver === "undefined") return () => {};
      const obs = new MutationObserver(avisar);
      obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
      return () => obs.disconnect();
    },
    () => (document.documentElement.dataset.theme === "claro" ? "claro" : "escuro"),
  );
}

function Bolinha({ cor }: { cor: string }) {
  return <span aria-hidden="true" className="inline-block h-2.5 w-2.5 shrink-0 rounded-full" ref={(el) => { if (el) el.style.backgroundColor = cor; }} />;
}

const BOTAO_DO_MAPA = "btn btn-glass h-7 min-w-7 px-1.5 text-[11px]";

export function GrafoPanel({ onAbrirGrande }: GrafoPanelProps) {
  const [grafo, setGrafo] = useState<Grafo | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [projeto, setProjeto] = useState<string>(TODOS);
  const [tipos, setTipos] = useState<TipoDeLigacao[]>(TODOS_OS_TIPOS);
  const [selecionada, setSelecionada] = useState<string | null>(null);
  const [termo, setTermo] = useState("");
  const mapa = useRef<GrafoCanvasHandle>(null);
  const tema = useTema();

  useEffect(() => {
    let vivo = true;
    invoke<Grafo>("memoria_grafo", { projeto: null })
      .then((g) => { if (vivo) setGrafo(g); })
      .catch((e: unknown) => { if (vivo) setErro(String(e)); });
    return () => { vivo = false; };
  }, []);

  const doProjeto = useMemo(() => (grafo ? filtrarGrafo(grafo, { projeto: projeto || null, tipos: TODOS_OS_TIPOS }) : null), [grafo, projeto]);
  const visivel = useMemo(() => (grafo ? filtrarGrafo(grafo, { projeto: projeto || null, tipos }) : null), [grafo, projeto, tipos]);
  const porTipo = useMemo(() => (doProjeto ? contarPorTipo(doProjeto) : null), [doProjeto]);
  const projetos = useMemo(() => {
    const contagem = new Map<string, number>();
    for (const n of grafo?.notas ?? []) if (n.projeto) contagem.set(n.projeto, (contagem.get(n.projeto) ?? 0) + 1);
    return [...contagem].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [grafo]);
  const paleta = useMemo(() => paletaDosProjetos((grafo?.notas ?? []).map((n) => n.projeto)), [grafo]);
  const achadas = useMemo(() => (visivel ? buscarNotas(visivel, termo) : []), [visivel, termo]);
  const destacadas = useMemo(() => new Set(achadas), [achadas]);

  if (erro) return <p role="alert" className="px-3 py-2 text-xs text-danger">Não foi possível ler a memória: {erro}</p>;
  if (!grafo || !visivel || !porTipo) return <p role="status" className="px-3 py-2 text-xs text-text-muted">Calculando as ligações. Na primeira vez pode demorar um pouco.</p>;
  if (grafo.notas.length === 0) {
    return (
      <div className="m-3 flex flex-col items-center gap-2 border-2 border-dashed border-border-subtle px-4 py-8 text-center">
        <ShapesIcon className="h-6 w-6 text-accent" aria-hidden="true" />
        <p className="text-xs font-medium text-text-primary">O grafo ainda está vazio</p>
        <p className="max-w-sm text-[11px] leading-relaxed text-text-muted">
          Ainda não há notas na memória. Elas nascem sozinhas: ao fim de cada turno, com a memória ligada em Conhecimento,
          o OMNI guarda até duas notas com o que vale lembrar, e as ligações aparecem aqui.
        </p>
      </div>
    );
  }

  const nota = visivel.notas.find((n) => n.slug === selecionada) ?? null;
  const doDestaque = nota ? vizinhas(visivel, nota.slug) : new Set<string>();
  const buscando = termo.trim() !== "";
  const listadas = buscando ? achadas.flatMap((slug) => visivel.notas.find((n) => n.slug === slug) ?? []) : visivel.notas;
  const corDe = (p: string) => corDoProjeto(p, tema, paleta);

  const opcoesDeProjeto: SelectOption<string>[] = [
    { value: TODOS, label: "Todos os projetos", hint: plural(grafo.notas.length, "nota", "notas") },
    ...projetos.map(([p, n]) => ({ value: p, label: p, hint: plural(n, "nota", "notas"), icon: <Bolinha cor={corDe(p)} /> })),
  ];

  const alternarTipo = (tipo: TipoDeLigacao) =>
    setTipos((atuais) => (atuais.includes(tipo) ? atuais.filter((t) => t !== tipo) : [...atuais, tipo]));
  const irPara = (slug: string) => { setSelecionada(slug); mapa.current?.focar(slug); };

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 px-3 pb-3 text-xs">
      {/* Barra de filtros: busca + projeto em cima, tipos de ligação como chips embaixo. */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-[10rem] flex-1">
          <SearchIcon aria-hidden="true" className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-text-muted" />
          <Input size="sm" type="search" value={termo} aria-label="Buscar nota no grafo" placeholder="Buscar nota pelo título…"
            className="!pl-7" onChange={(e) => setTermo(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && achadas[0]) { e.preventDefault(); irPara(achadas[0]); }
              if (e.key === "Escape" && termo) { e.preventDefault(); setTermo(""); }
            }} />
        </div>
        <Select<string> size="sm" aria-label="Projeto" className="w-44 max-w-full" value={projeto}
          options={opcoesDeProjeto} searchable={projetos.length > BUSCA_COM_LISTA} searchPlaceholder="Buscar projeto…"
          onChange={(p) => { setProjeto(p); setSelecionada(null); }} />
        {onAbrirGrande && (
          <button type="button" onClick={onAbrirGrande} className="btn btn-glass h-8 px-2 text-[11px]">Abrir em aba grande</button>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Tipos de ligação">
        {TIPOS_DE_LIGACAO.map(({ tipo, rotulo }) => (
          <button key={tipo} type="button" aria-pressed={tipos.includes(tipo)} onClick={() => alternarTipo(tipo)}
            title={tipos.includes(tipo) ? "Esconder este tipo de ligação" : "Mostrar este tipo de ligação"}
            className="inline-flex h-6 items-center gap-1.5 border-2 border-border-subtle px-2 text-[10px] font-semibold text-text-muted line-through decoration-text-muted/60 transition-colors hover:text-text-secondary aria-pressed:border-accent/70 aria-pressed:bg-accent/15 aria-pressed:text-text-primary aria-pressed:no-underline">
            <span aria-hidden="true" className="h-1.5 w-1.5 bg-current opacity-60" />
            {rotulo}
            <span className="tabular-nums opacity-70">{porTipo[tipo]}</span>
          </button>
        ))}
        <span className="ml-auto whitespace-nowrap text-[11px] tabular-nums text-text-muted">
          {plural(visivel.notas.length, "nota", "notas")} · {plural(visivel.ligacoes.length, "ligação", "ligações")}
        </span>
      </div>

      {/* Mapa: ocupa o que sobrar da altura; controles e legenda flutuam por cima. */}
      <div className="relative min-h-[18rem] flex-1 overflow-hidden border-2 border-border-subtle bg-bg-primary">
        <GrafoCanvas ref={mapa} grafo={visivel} selecionada={selecionada} destacadas={destacadas} paleta={paleta} aoSelecionar={setSelecionada} />
        {buscando && achadas.length === 0 && (
          <div className="absolute inset-x-0 top-3 mx-auto flex w-fit items-center gap-2 border-2 border-border bg-bg-elevated px-3 py-1.5 shadow-lg">
            <span className="text-text-secondary">Nenhuma nota com “{termo.trim()}”.</span>
            <button type="button" className="font-semibold text-accent hover:underline" onClick={() => setTermo("")}>Limpar busca</button>
          </div>
        )}
        {buscando && achadas.length > 0 && (
          <p role="status" className="absolute left-2 top-2 bg-bg-elevated/90 px-2 py-0.5 text-[11px] text-text-secondary">
            {plural(achadas.length, "nota encontrada", "notas encontradas")} · Enter vai até a primeira
          </p>
        )}
        <div className="absolute bottom-2 right-2 flex gap-1">
          <button type="button" className={BOTAO_DO_MAPA} aria-label="Aproximar" title="Aproximar" onClick={() => mapa.current?.aproximar(1.4)}>+</button>
          <button type="button" className={BOTAO_DO_MAPA} aria-label="Afastar" title="Afastar" onClick={() => mapa.current?.aproximar(1 / 1.4)}>−</button>
          <button type="button" className={BOTAO_DO_MAPA} aria-label="Centralizar o grafo" title="Enquadrar o grafo inteiro" onClick={() => mapa.current?.centralizar()}>Centralizar</button>
        </div>
        {projetos.length > 1 && !projeto && !onAbrirGrande && (
          <ul aria-label="Cores dos projetos" className="pointer-events-none absolute bottom-2 left-2 max-w-[50%] space-y-0.5 bg-bg-primary/80 px-2 py-1 text-[10px] text-text-secondary">
            {projetos.slice(0, MAX_NA_LEGENDA).map(([p]) => (
              <li key={p} className="flex items-center gap-1.5 truncate"><Bolinha cor={corDe(p)} /><span className="truncate">{p}</span></li>
            ))}
            {projetos.length > MAX_NA_LEGENDA && <li className="text-text-muted">+{projetos.length - MAX_NA_LEGENDA} projetos</li>}
          </ul>
        )}
      </div>

      {nota && (
        <section aria-label="Nota selecionada" className="space-y-1.5 border-2 border-border-subtle bg-bg-elevated p-3">
          <div className="flex items-start gap-2">
            {nota.projeto && <span className="mt-1"><Bolinha cor={corDe(nota.projeto)} /></span>}
            <h3 className="min-w-0 flex-1 font-semibold text-text-primary">{nota.titulo}</h3>
            <button type="button" aria-label="Fechar detalhe da nota" className="text-text-muted hover:text-text-primary" onClick={() => setSelecionada(null)}>
              <CloseIcon className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </div>
          {nota.projeto && <p className="text-[11px] text-text-muted">{nota.projeto}</p>}
          <p className="max-h-40 overflow-auto whitespace-pre-wrap leading-relaxed text-text-secondary">{nota.resumo}</p>
          {nota.tags.length > 0 && (
            <p className="flex flex-wrap gap-1">{nota.tags.map((t) => <span key={t} className="bg-overlay/[0.07] px-1.5 py-0.5 text-[10px] text-text-secondary">#{t}</span>)}</p>
          )}
          {doDestaque.size > 0 && (
            <p className="text-text-muted">
              Ligada a:{" "}
              {visivel.notas.filter((n) => doDestaque.has(n.slug)).map((n) => <span key={n.slug} className="mr-2 text-text-primary">{n.titulo}</span>)}
            </p>
          )}
        </section>
      )}

      <ul aria-label="Notas da memória" className="max-h-40 shrink-0 space-y-px overflow-auto">
        {listadas.map((n) => (
          <li key={n.slug}>
            <button type="button" aria-pressed={n.slug === selecionada} data-vizinha={doDestaque.has(n.slug)}
              onClick={() => (n.slug === selecionada ? setSelecionada(null) : irPara(n.slug))}
              className="flex w-full items-center gap-2 px-1.5 py-1 text-left text-text-secondary hover:bg-overlay/[0.07] aria-pressed:bg-accent/10 aria-pressed:text-accent data-[vizinha=true]:text-text-primary">
              {n.projeto ? <Bolinha cor={corDe(n.projeto)} /> : <span aria-hidden="true" className="h-2.5 w-2.5 shrink-0 rounded-full border border-text-muted" />}
              <span className="min-w-0 flex-1 truncate">{n.titulo}</span>
              {n.projeto && <span className="shrink-0 text-[10px] text-text-muted">{n.projeto}</span>}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

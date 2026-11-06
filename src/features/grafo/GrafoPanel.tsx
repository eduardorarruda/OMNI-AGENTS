import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { GrafoCanvas } from "./GrafoCanvas";
import { filtrarGrafo, TIPOS_DE_LIGACAO, vizinhas, type Grafo, type TipoDeLigacao } from "./grafoModel";

interface GrafoPanelProps {
  /** Presente só no painel da lateral: abre o mesmo grafo numa aba grande. */
  onAbrirGrande?: () => void;
}

const TODOS_OS_TIPOS = TIPOS_DE_LIGACAO.map((t) => t.tipo);

export function GrafoPanel({ onAbrirGrande }: GrafoPanelProps) {
  const [grafo, setGrafo] = useState<Grafo | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [projeto, setProjeto] = useState<string | null>(null);
  const [tipos, setTipos] = useState<TipoDeLigacao[]>(TODOS_OS_TIPOS);
  const [selecionada, setSelecionada] = useState<string | null>(null);

  useEffect(() => {
    let vivo = true;
    invoke<Grafo>("memoria_grafo", { projeto: null })
      .then((g) => { if (vivo) setGrafo(g); })
      .catch((e: unknown) => { if (vivo) setErro(String(e)); });
    return () => { vivo = false; };
  }, []);

  const visivel = useMemo(() => (grafo ? filtrarGrafo(grafo, { projeto, tipos }) : null), [grafo, projeto, tipos]);
  const projetos = useMemo(() => [...new Set((grafo?.notas ?? []).map((n) => n.projeto).filter(Boolean))].sort(), [grafo]);
  const doDestaque = visivel && selecionada ? vizinhas(visivel, selecionada) : new Set<string>();
  const nota = visivel?.notas.find((n) => n.slug === selecionada) ?? null;

  if (erro) return <p role="alert" className="px-3 py-2 text-xs text-danger">Não foi possível ler a memória: {erro}</p>;
  if (!grafo || !visivel) return <p role="status" className="px-3 py-2 text-xs text-text-muted">Calculando as ligações. Na primeira vez pode demorar um pouco.</p>;
  if (grafo.notas.length === 0) {
    return (
      <p className="px-3 py-2 text-xs text-text-muted">
        Ainda não há notas na memória. Elas nascem sozinhas: ao fim de cada turno, com a memória ligada em Conhecimento,
        o OMNI guarda até duas notas com o que vale lembrar, e as ligações aparecem aqui.
      </p>
    );
  }

  const alternarTipo = (tipo: TipoDeLigacao) =>
    setTipos((atuais) => (atuais.includes(tipo) ? atuais.filter((t) => t !== tipo) : [...atuais, tipo]));

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 px-3 pb-2 text-xs">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <label className="flex items-center gap-1 text-text-muted">
          Projeto
          <select value={projeto ?? ""} onChange={(e) => setProjeto(e.target.value || null)}
            className="border border-border-subtle bg-bg-elevated px-1 py-0.5 text-text-primary">
            <option value="">Todos</option>
            {projetos.map((p) => <option key={p} value={p}>{p}</option>)}
          </select>
        </label>
        {TIPOS_DE_LIGACAO.map(({ tipo, rotulo }) => (
          <label key={tipo} className="flex items-center gap-1 text-text-muted">
            <input type="checkbox" checked={tipos.includes(tipo)} onChange={() => alternarTipo(tipo)} />
            {rotulo}
          </label>
        ))}
        {onAbrirGrande && (
          <button type="button" onClick={onAbrirGrande}
            className="border border-border-subtle bg-bg-elevated px-2 py-0.5 text-text-primary hover:bg-overlay/[0.07]">
            Abrir em aba grande
          </button>
        )}
      </div>
      <div className="relative min-h-48 flex-1 border border-border-subtle">
        <GrafoCanvas grafo={visivel} selecionada={selecionada} aoSelecionar={setSelecionada} />
      </div>
      {nota && (
        <section aria-label="Nota selecionada" className="space-y-1 border border-border-subtle bg-bg-elevated p-2">
          <h3 className="font-medium text-text-primary">{nota.titulo}</h3>
          <p className="whitespace-pre-wrap text-text-secondary">{nota.resumo}</p>
          {nota.tags.length > 0 && <p className="text-text-muted">Tags: {nota.tags.join(", ")}</p>}
          {doDestaque.size > 0 && (
            <p className="text-text-muted">
              Ligada a:{" "}
              {visivel.notas.filter((n) => doDestaque.has(n.slug)).map((n) => <span key={n.slug} className="mr-2 text-text-primary">{n.titulo}</span>)}
            </p>
          )}
        </section>
      )}
      <ul aria-label="Notas da memória" className="max-h-40 space-y-0.5 overflow-auto">
        {visivel.notas.map((n) => (
          <li key={n.slug}>
            <button type="button" aria-pressed={n.slug === selecionada} data-vizinha={doDestaque.has(n.slug)}
              onClick={() => setSelecionada(n.slug === selecionada ? null : n.slug)}
              className="w-full truncate px-1 py-0.5 text-left text-text-secondary hover:bg-overlay/[0.07] aria-pressed:text-accent data-[vizinha=true]:text-text-primary">
              {n.titulo}{n.projeto ? ` · ${n.projeto}` : ""}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

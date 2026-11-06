import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { AgentIcon } from "../../components/ui/AgentIcon";
import { usageTone } from "../../lib/uso";
import { NOME_DO_AGENTE } from "../../mobile/api";
import type { Usage, UsageWindow } from "./AccountUsage";

/** Contagem de tokens de uma janela (espelha `omni_core::tokens::Contagem`). */
export interface ContagemDeTokens { entrada: number; saida: number; cache: number }
/** Tokens somados dos históricos locais. Campo `null` = a fonte não tem o dado. */
export interface TokensDaConta { hoje: ContagemDeTokens | null; sete_dias: ContagemDeTokens | null; motivo: string | null }

export interface ContaDoDetalhe { nome: string; email?: string | null; provider: string }

/** Marca o botão que abre o detalhe: clicar nele de novo não conta como "clique fora". */
export const GATILHO_DO_DETALHE = "data-abre-detalhe";

const SITUACAO: Record<string, string> = {
  available: "Medido", stale: "Desatualizado", unavailable: "Indisponível", unknown: "Sem leitura ainda",
};

const NAO_DISPONIVEL = "não disponível";
const numero = (n: number) => n.toLocaleString("pt-BR");
const dataHora = (segundos: number) =>
  new Date(segundos * 1000).toLocaleString("pt-BR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });

function renovacao(janela: UsageWindow): string | null {
  if (janela.resets_at) return `Renova em ${dataHora(janela.resets_at)}`;
  return janela.reset_label ? `Renova ${janela.reset_label.replace(/^Resets\s+/, "")}` : null;
}

/** Um medidor por janela. Sem a janela, escreve "não disponível": 0% seria dado inventado. */
function Janela({ rotulo, janela }: { rotulo: string; janela: UsageWindow | null }) {
  const usado = janela ? Math.min(100, Math.max(0, janela.used_percent)) : 0;
  const tom = janela ? usageTone(janela.used_percent) : null;
  const sub = janela ? renovacao(janela) : null;
  return (
    <div role="group" aria-label={rotulo} className="flex flex-col gap-1">
      <span className="flex items-baseline justify-between gap-2">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-secondary">{rotulo}</span>
        {janela
          ? <span className={`text-[15px] font-bold ${tom?.text}`}>{`${janela.used_percent.toLocaleString("pt-BR")}%`}</span>
          : <span className="text-text-muted">{NAO_DISPONIVEL}</span>}
      </span>
      {janela && (
        <span role="progressbar" aria-label={`${rotulo} consumido`} aria-valuenow={Math.round(usado)} aria-valuemin={0} aria-valuemax={100} className="uso-barra">
          <span className={tom?.bar} style={{ width: `${usado}%` }} />
        </span>
      )}
      {sub && <span className="text-[10px] text-text-muted">{sub}</span>}
    </div>
  );
}

function LinhaDeTokens({ rotulo, contagem }: { rotulo: string; contagem: ContagemDeTokens | null }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="text-text-secondary">{rotulo}</span>
      {contagem
        ? <span className="text-right text-text-primary">
          {numero(contagem.entrada)} entrada · {numero(contagem.saida)} saída · {numero(contagem.cache)} cache
        </span>
        : <span className="text-text-muted">{NAO_DISPONIVEL}</span>}
    </div>
  );
}

interface DetalheDaContaProps {
  uso: Usage | null;
  /** `undefined` = ainda lendo; `null` = sem fonte. */
  tokens: TokensDaConta | null | undefined;
  conta: ContaDoDetalhe;
  onFechar: () => void;
}

/**
 * Detalhe de uma conta: janela de 5 horas e limite semanal em medidores separados, com renovação,
 * situação e motivo, mais os tokens somados dos históricos locais. É o mesmo painel do rodapé e do
 * botão "Uso" da barra de cima. Esc e clique fora fecham, e só ele (o painel de baixo continua aberto).
 */
export function DetalheDaConta({ uso, tokens, conta, onFechar }: DetalheDaContaProps) {
  const raiz = useRef<HTMLDivElement>(null);
  const fechar = useRef(onFechar);
  fechar.current = onFechar;

  // Captura na janela: o painel do topo também ouve Esc e clique fora, e fechar os dois de uma vez
  // seria surpresa.
  useEffect(() => {
    const esc = (evento: KeyboardEvent) => {
      if (evento.key !== "Escape") return;
      evento.stopImmediatePropagation();
      fechar.current();
    };
    const fora = (evento: MouseEvent) => {
      const alvo = evento.target as Element | null;
      if (raiz.current?.contains(alvo) || alvo?.closest?.(`[${GATILHO_DO_DETALHE}]`)) return;
      evento.stopPropagation();
      fechar.current();
    };
    window.addEventListener("keydown", esc, true);
    window.addEventListener("mousedown", fora, true);
    return () => { window.removeEventListener("keydown", esc, true); window.removeEventListener("mousedown", fora, true); };
  }, []);

  const nome = NOME_DO_AGENTE[conta.provider] ?? conta.provider;
  const situacao = uso ? SITUACAO[uso.status] ?? uso.status : "Lendo…";
  const motivo = uso?.reason ?? null;
  const emTokens = tokens === undefined ? null : tokens;
  return createPortal(
    <div
      ref={raiz}
      role="dialog"
      aria-label={`Detalhe da conta ${conta.nome}`}
      style={{ left: "50%", top: "50%", transform: "translate(-50%, -50%)", zIndex: 210 }}
      className="uso-painel gap-3 p-4"
    >
      <header className="flex items-center gap-2">
        <AgentIcon provider={conta.provider} size={14} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-bold text-text-primary">{conta.nome}</span>
          <span className="block truncate text-[10px] text-text-muted">
            {conta.email && <><span>{conta.email}</span> · </>}{nome}
          </span>
        </span>
        <span className="shrink-0 text-[10px] font-bold uppercase tracking-wider text-text-secondary">{situacao}</span>
      </header>
      <Janela rotulo="Janela de 5 horas" janela={uso?.primary ?? null} />
      <Janela rotulo="Limite semanal" janela={uso?.secondary ?? null} />
      {motivo && <p role="alert" className="text-warning">{motivo}</p>}
      {uso?.observed_at_ms ? <p className="text-[10px] text-text-muted">Lido em {new Date(uso.observed_at_ms).toLocaleString("pt-BR")}</p> : null}
      <section aria-label="Tokens" className="flex flex-col gap-1 border-t border-border-subtle pt-3">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-secondary">Tokens (históricos locais)</span>
        {tokens === undefined
          ? <span className="text-text-muted">Lendo…</span>
          : <>
            <LinhaDeTokens rotulo="Hoje" contagem={emTokens?.hoje ?? null} />
            <LinhaDeTokens rotulo="Últimos 7 dias" contagem={emTokens?.sete_dias ?? null} />
            {emTokens?.motivo && <span className="text-[10px] text-text-muted">{emTokens.motivo}</span>}
          </>}
      </section>
    </div>,
    document.body,
  );
}

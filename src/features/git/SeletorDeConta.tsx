import { useEffect, useState } from "react";
import { ghContas } from "./gitService";

interface Props {
  /** Conta escolhida para o workspace; `null` = a conta ativa do `gh`. */
  atual: string | null;
  /** Canto inferior esquerdo do popover, em coordenadas de tela: o rodapé fica colado embaixo. */
  ancora: { left: number; bottom: number };
  onEscolher: (conta: string | null) => void;
  onFechar: () => void;
}

/** Seletor da conta do GitHub deste workspace, aberto a partir do rodapé. A lista é relida a cada
 *  abertura: conta adicionada com `gh auth login` aparece sem reabrir o app. */
export function SeletorDeConta({ atual, ancora, onEscolher, onFechar }: Props) {
  const [contas, setContas] = useState<string[] | null>(null);
  useEffect(() => {
    let vivo = true;
    ghContas().then((lidas) => { if (vivo) setContas(lidas); }).catch(() => { if (vivo) setContas([]); });
    return () => { vivo = false; };
  }, []);

  const opcao = "w-full truncate px-2 py-1 text-left text-xs text-text-secondary hover:bg-overlay/[0.07] disabled:opacity-40";
  const sumiu = atual !== null && contas !== null && !contas.includes(atual);
  return (
    <>
      {/* Camada de fundo: clicar fora fecha, sem listener global no document. */}
      <span className="fixed inset-0 z-40" onPointerDown={onFechar} />
      <div role="dialog" aria-label="Conta do GitHub deste workspace" style={{ left: ancora.left, bottom: ancora.bottom }}
        className="popover fixed z-50 flex w-64 flex-col gap-1 p-1" onKeyDown={(event) => { if (event.key === "Escape") onFechar(); }}>
        <ul className="max-h-48 overflow-y-auto">
          <li>
            <button type="button" autoFocus disabled={atual === null} className={opcao} onClick={() => onEscolher(null)}>
              {atual === null ? "● " : "○ "}Conta ativa do gh
            </button>
          </li>
          {contas === null && <li className="px-2 py-1 text-xs text-text-muted">Lendo contas…</li>}
          {contas?.map((conta) => (
            <li key={conta}>
              <button type="button" disabled={conta === atual} className={opcao} onClick={() => onEscolher(conta)}>
                {conta === atual ? "● " : "○ "}{conta}
              </button>
            </li>
          ))}
        </ul>
        {contas?.length === 0 && <p className="px-2 py-1 text-xs text-text-muted">Nenhuma conta logada. Rode `gh auth login` no terminal.</p>}
        {sumiu && <p className="px-2 py-1 text-xs text-warning">{atual} não está mais logada no gh: os terminais abrem com a conta ativa.</p>}
        <p className="px-2 py-1 text-[10px] text-text-muted">Vale para terminais e agentes abertos depois da troca.</p>
      </div>
    </>
  );
}

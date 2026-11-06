import { useExecucoesAtivas } from "../fluxos/useExecucoesAtivas";
import { useSegundoPlano } from "../segundoPlano/useSegundoPlano";

interface Props { projetoId: string; onAbrir: () => void }

/** Faixa no topo do projeto: o que está rodando nele agora. Some quando não há nada; o clique abre o panorama. */
export function FaixaDeExecucao({ projetoId, onAbrir }: Props) {
  const { ativas } = useExecucoesAtivas();
  const { itens } = useSegundoPlano(projetoId);
  const fluxos = ativas.filter((e) => e.projeto_id === projetoId).length;
  // Fluxo já vem contado em `ativas`; aqui entram só sessões e subagentes, para não contar duas vezes.
  const emFundo = itens.filter((i) => i.projeto_id === projetoId && i.tipo !== "fluxo").length;
  if (fluxos + emFundo === 0) return null;

  const partes = [
    fluxos > 0 && `${fluxos} ${fluxos === 1 ? "fluxo" : "fluxos"}`,
    emFundo > 0 && `${emFundo} em segundo plano`,
  ].filter(Boolean).join(" · ");

  return (
    <button type="button" className="faixa-execucao" onClick={onAbrir} aria-label={`${partes} rodando neste projeto. Abrir o panorama`}
      title="Abrir o panorama do que roda neste projeto">
      <span aria-hidden className="faixa-execucao-ponto" />
      <span>{partes}</span>
      <span className="faixa-execucao-ver">rodando neste projeto · ver</span>
    </button>
  );
}

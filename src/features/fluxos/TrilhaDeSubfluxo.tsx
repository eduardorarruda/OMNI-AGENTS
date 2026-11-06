import { Fragment, useEffect, useState } from "react";
import { Select } from "../../components/ui";
import { obterExecucao, type Execucao, type FluxoSalvo } from "./fluxosService";

/** Ids dos ancestrais, do pai para cima. `listar` não traz a cadeia e `pilha` só tem nomes, então sobe
 *  por `obter`. Ancestral que sumiu corta a subida: o que já achou continua valendo. */
async function ancestraisDe(execucao: Execucao): Promise<string[]> {
  const ids: string[] = [];
  let pai = execucao.pai;
  while (pai) {
    ids.push(pai.execucao);
    pai = await obterExecucao(pai.execucao).then((x) => x.pai, () => null);
  }
  return ids;
}

/** `X › Y › Z` de uma execução filha: cada ancestral abre a execução dele, e a última é a aberta. */
export function TrilhaDeSubfluxo({ execucao, onAbrir }: { execucao: Execucao; onAbrir: (id: string) => void }) {
  const [ancestrais, setAncestrais] = useState<string[]>([]);
  const paiId = execucao.pai?.execucao;
  useEffect(() => {
    let vivo = true;
    setAncestrais([]);
    if (paiId) void ancestraisDe({ ...execucao, pai: { execucao: paiId, no: "" } }).then((ids) => { if (vivo) setAncestrais(ids); });
    return () => { vivo = false; };
    // `execucao` muda a cada leitura; a cadeia só muda com o pai.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [execucao.id, paiId]);

  const pilha = execucao.pilha ?? [];
  if (pilha.length === 0) return null;
  const ultimo = pilha.length - 1;
  return (
    <nav className="fx-trilha" aria-label="Cadeia de subfluxos">
      {pilha.map((nome, i) => {
        // O pai é o penúltimo da pilha, o avô o antepenúltimo: conta a partir do fim.
        const id = i === ultimo ? undefined : ancestrais[ultimo - 1 - i];
        return (
          <Fragment key={i}>
            {i > 0 && <span className="fx-trilha-seta" aria-hidden> › </span>}
            {id ? <button type="button" className="fx-trilha-item" onClick={() => onAbrir(id)}>{nome}</button>
              : <span className={i === ultimo ? "fx-trilha-atual" : "fx-trilha-item"} aria-current={i === ultimo ? "page" : undefined}>{nome}</span>}
          </Fragment>
        );
      })}
    </nav>
  );
}

/** O fluxo que o nó chama. O atual fica na lista, marcado: chamar a si mesmo é permitido, só avisado. */
export function SeletorDeSubfluxo({ id, fluxos, atualId, valor, onMudar }: {
  id: string; fluxos: FluxoSalvo[]; atualId?: string; valor: string; onMudar: (fluxo: string) => void;
}) {
  // Mesmo id nos dois escopos: o engine roda o do projeto.
  const visiveis = fluxos.filter((f) => f.escopo === "projeto" || !fluxos.some((o) => o.escopo === "projeto" && o.id === f.id));
  const opcoes = visiveis.map((f) => ({ value: f.id, label: f.nome, hint: f.id === atualId ? "Este fluxo: forma um ciclo" : f.escopo === "global" ? "Global" : "Projeto" }));
  return (
    <Select id={id} size="sm" value={valor} onChange={onMudar} searchable placeholder={valor ? `Não encontrado: ${valor}` : "Escolha…"}
      options={[...(valor ? [] : [{ value: "", label: "Escolha…" }]), ...opcoes]} />
  );
}

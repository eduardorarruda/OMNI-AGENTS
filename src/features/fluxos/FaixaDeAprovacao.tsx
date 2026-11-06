import { useEffect, useState } from "react";
import { Button } from "../../components/ui/Button";
import { perguntaDoNo } from "./execucaoModel";
import { aprovarNo, type Execucao } from "./fluxosService";
import { useExecucoesAtivas } from "./useExecucoesAtivas";

export interface Pendencia { chave: string; execucao: string; no: string; fluxo: string; texto: string }

/** Cada nó parado esperando você, de todas as execuções: com ramos em paralelo, uma execução pode ter vários. */
export function pendenciasDe(execucoes: Execucao[]): Pendencia[] {
  return execucoes.flatMap((e) => e.fluxo.nos
    .filter((n) => e.nos[n.id]?.status === "aguardando")
    .map((n) => ({ chave: `${e.id}:${n.id}`, execucao: e.id, no: n.id, fluxo: e.fluxo.nome, texto: perguntaDoNo(n) })));
}

const mensagem = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Faixa fixa no topo da tela de Fluxos: o que espera aprovação, com Aprovar e Recusar grandes.
 * Sem pendência, não existe. Se o engine recusar a resposta, o erro fica na faixa.
 */
export function FaixaDeAprovacao() {
  const { aguardando } = useExecucoesAtivas();
  const [enviando, setEnviando] = useState<string | null>(null);
  const [respondidas, setRespondidas] = useState<ReadonlySet<string>>(new Set());
  const [erro, setErro] = useState<{ chave: string; texto: string } | null>(null);
  const todas = pendenciasDe(aguardando);

  // Respondida some da faixa já; quando o engine confirma que o nó saiu de "aguardando", esquece (o nó pode voltar num laço).
  useEffect(() => {
    setRespondidas((atual) => {
      const vivas = new Set(todas.map((p) => p.chave));
      const resto = [...atual].filter((c) => vivas.has(c));
      return resto.length === atual.size ? atual : new Set(resto);
    });
  }, [todas.map((p) => p.chave).join("|")]); // eslint-disable-line react-hooks/exhaustive-deps

  async function responder(p: Pendencia, sim: boolean) {
    setEnviando(p.chave); setErro(null);
    try {
      await aprovarNo(p.execucao, p.no, sim);
      setRespondidas((atual) => new Set(atual).add(p.chave));
    } catch (motivo) {
      setErro({ chave: p.chave, texto: mensagem(motivo) });
    } finally {
      setEnviando(null);
    }
  }

  const visiveis = todas.filter((p) => !respondidas.has(p.chave));
  if (visiveis.length === 0) return null;
  return (
    <div className="fx-faixa-aprovacao" role="region" aria-label="Aprovações pendentes">
      {visiveis.map((p) => (
        <div key={p.chave} className="fx-faixa-item">
          <div className="fx-faixa-texto">
            <span className="fx-rotulo">Esperando você · {p.fluxo}</span>
            <b>{p.texto}</b>
            {erro?.chave === p.chave && <p role="alert" className="fx-erro">{erro.texto}</p>}
          </div>
          <div className="fx-faixa-acoes">
            <Button variant="ghost" disabled={enviando === p.chave} aria-label={`Recusar: ${p.texto}`} onClick={() => void responder(p, false)}>Recusar</Button>
            <Button variant="primary" disabled={enviando === p.chave} aria-label={`Aprovar: ${p.texto}`} onClick={() => void responder(p, true)}>Aprovar</Button>
          </div>
        </div>
      ))}
    </div>
  );
}

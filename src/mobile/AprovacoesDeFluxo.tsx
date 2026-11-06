import { useEffect, useRef, useState } from "react";
import { Button } from "../components/ui/Button";
import { perguntaDoNo } from "../features/fluxos/execucaoModel";
import { request } from "./api";
import { avisarFluxos, novosAvisosDeFluxo, type AvisoDeFluxo } from "./notificacoes";

/** Só o que esta tela lê da execução (o tipo completo mora em `features/fluxos/fluxosService.ts`, que é do PC). */
interface Execucao {
  id: string; status: string; entrada: string;
  fluxo: { nome: string; nos: { id: string; tipo: string; titulo: string; config: Record<string, unknown> }[]; conexoes: { de: string; para: string }[] };
  nos: Record<string, { status: string; saida: string }>;
}
const chaveDe = (p: { execucao: string; no: string }) => `${p.execucao}:${p.no}`;
export interface Pendente { execucao: string; no: string; fluxo: string; pergunta: string; entrada: string; anterior: string }

/** Todo nó parado esperando você em cada execução que espera você (ramos em paralelo podem ter vários), com a saída da etapa que levou até ele. */
export function pendentesDe(execucoes: Execucao[]): Pendente[] {
  return execucoes.flatMap((e) => {
    return e.fluxo.nos.filter((n) => e.nos[n.id]?.status === "aguardando").map((no) => {
      const antes = e.fluxo.conexoes.map((c) => c.para === no.id ? e.nos[c.de] : undefined).find((n) => n?.saida);
      return {
        execucao: e.id, no: no.id, fluxo: e.fluxo.nome, entrada: e.entrada, anterior: antes?.saida.slice(-1500) ?? "",
        pergunta: perguntaDoNo(no),
      };
    });
  });
}

/**
 * Fluxo parado num nó "Aprovação": aparece em "Precisa de você" da web e do celular, com Aprovar e
 * Recusar. Antes só dava para responder no PC. Engine sem as rotas de fluxo: não mostra nada.
 */
export function AprovacoesDeFluxo({ aoMudar }: { aoMudar?: (quantas: number) => void }) {
  const [pendentes, setPendentes] = useState<Pendente[]>([]);
  const [respondendo, setRespondendo] = useState<string | null>(null);
  const [erro, setErro] = useState<{ chave: string; texto: string } | null>(null);
  const avisosVistos = useRef(new Map<string, number>());
  const primeiraLeitura = useRef(true);

  useEffect(() => {
    let vivo = true;
    async function ler() {
      try {
        const lista = await request<(AvisoDeFluxo & { status: string })[]>("/fluxos/execucoes");
        avisarFluxos(novosAvisosDeFluxo(lista, avisosVistos.current, primeiraLeitura.current));
        primeiraLeitura.current = false;
        const detalhes = await Promise.all(lista.filter((e) => e.status === "aguardando")
          .map((e) => request<Execucao>(`/fluxos/execucoes/${encodeURIComponent(e.id)}`)));
        if (vivo) setPendentes(pendentesDe(detalhes));
      } catch { /* engine antigo ou fora do ar: a lista de conversas já avisa */ }
    }
    void ler();
    const timer = window.setInterval(() => void ler(), 5000);
    return () => { vivo = false; window.clearInterval(timer); };
  }, []);
  useEffect(() => { aoMudar?.(pendentes.length); }, [pendentes.length, aoMudar]);

  async function responder(p: Pendente, sim: boolean) {
    const chave = chaveDe(p);
    setRespondendo(chave); setErro(null);
    try {
      await request(`/fluxos/execucoes/${encodeURIComponent(p.execucao)}/nos/${encodeURIComponent(p.no)}/aprovar`, {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sim }),
      });
      setPendentes((atual) => atual.filter((x) => chaveDe(x) !== chave));
    } catch (motivo) { setErro({ chave, texto: motivo instanceof Error ? motivo.message : String(motivo) }); }
    finally { setRespondendo(null); }
  }

  if (pendentes.length === 0) return null;
  return (
    <ul className="aprov-fluxo" aria-label="Fluxos esperando aprovação">
      {pendentes.map((p) => (
        <li key={chaveDe(p)} className="aprov-fluxo-item glass">
          <span className="aprov-fluxo-rotulo">Fluxo · {p.fluxo}</span>
          <b className="aprov-fluxo-pergunta">{p.pergunta}</b>
          {p.entrada && <span className="aprov-fluxo-entrada">{p.entrada}</span>}
          {p.anterior && <details><summary>O que a etapa anterior entregou</summary><pre>{p.anterior}</pre></details>}
          {erro?.chave === chaveDe(p) && <p role="alert" className="text-xs text-danger">{erro.texto}</p>}
          <div className="aprov-fluxo-acoes">
            <Button size="sm" variant="ghost" disabled={respondendo !== null} onClick={() => void responder(p, false)}>Recusar</Button>
            <Button size="sm" variant="primary" disabled={respondendo !== null} onClick={() => void responder(p, true)}>
              {respondendo === chaveDe(p) ? "Enviando…" : "Aprovar"}
            </Button>
          </div>
        </li>
      ))}
    </ul>
  );
}

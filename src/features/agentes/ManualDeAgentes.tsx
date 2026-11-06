import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { CloseIcon } from "../../components/ui/PixelIcon";

interface Secao { id: string; titulo: string; imagem?: string; legenda?: string; corpo: ReactNode }

const SECOES: Secao[] = [
  {
    id: "visao", titulo: "Como tudo se encaixa",
    corpo: <>
      <p>O OMNI monta <b>times de agentes</b> em duas peças:</p>
      <ol>
        <li><b>Modelos de agentes</b>: cada um é um especialista (QA, revisor, dev…) salvo num arquivo <code>.md</code> — papel, instruções, IA, modelo, esforço e ferramentas.</li>
        <li><b>Fluxos</b>: o caminho que os agentes percorrem, montado num canvas como no n8n. Cada caixa é uma etapa; as setas dizem quem vem depois.</li>
      </ol>
      <p>Ao executar, cada agente trabalha <b>sem tela</b> numa cópia isolada do seu repositório (um <i>git worktree</i>, numa branch nova). Sua pasta não é tocada, e você acompanha tudo ao vivo.</p>
    </>,
  },
  {
    id: "modelos", titulo: "Modelos de agentes", imagem: "/manual/modelos.webp", legenda: "Configurações → Modelos de agentes",
    corpo: <>
      <p>Em <b>Configurações → Modelos de agentes</b> ficam os agentes salvos. O OMNI já vem com 7: orquestrador, arquiteto, dev, QA, revisor, segurança e documentação.</p>
      <ul>
        <li><b>Global</b> vale em todo projeto (<code>~/.omni/agents</code>). <b>Projeto</b> fica em <code>.omni/agents</code> e vai no git — o time inteiro recebe.</li>
        <li><b>No Claude Code</b> copia o agente para <code>.claude/agents</code>: o próprio Claude passa a usá-lo como subagente.</li>
      </ul>
    </>,
  },
  {
    id: "criar-agente", titulo: "Criar um agente com IA", imagem: "/manual/criar-agente.webp", legenda: "Descreva o agente em uma frase",
    corpo: <>
      <p>Descreva o que o agente faz — <i>“um agente que escreve testes end-to-end com Playwright para os fluxos críticos”</i> — e clique em <b>Criar com IA</b>.</p>
      <p>A IA segue as práticas dos times de agentes do mercado: <b>uma responsabilidade</b>, descrição que diz quando chamar, <b>ferramentas mínimas</b>, passos com verificação, critério de pronto, limites e o <b>formato de saída</b> que o próximo agente vai ler. Ela também escolhe o modelo e o esforço certos para o tipo de trabalho.</p>
      <p>O resultado abre no editor para você revisar antes de salvar. Dentro de um fluxo, o painel do nó também tem <b>Criar novo agente com IA</b>.</p>
    </>,
  },
  {
    id: "editar-agente", titulo: "Editar: IA, modelo, esforço e saída", imagem: "/manual/editor-agente.webp", legenda: "O editor de um modelo",
    corpo: <>
      <ul>
        <li><b>CLI</b>: quem roda o agente — Claude Code ou Codex.</li>
        <li><b>Modelo</b> e <b>esforço</b>: Opus/Fable com esforço alto para arquitetura, revisão e segurança; Sonnet/médio para implementação e QA; Haiku/baixo para tarefas curtas. Mais esforço = mais qualidade e mais tokens.</li>
        <li><b>Saída</b>: <i>veredito</i> (aprova ou reprova — use antes de uma Condição), <i>plano</i> (divide o trabalho), <i>relatório</i> ou <i>texto</i>.</li>
        <li><b>Ferramentas</b>: quem só analisa não precisa de Edit nem Write.</li>
      </ul>
    </>,
  },
  {
    id: "fluxo", titulo: "Montar um fluxo", imagem: "/manual/fluxo-editor.webp", legenda: "Fluxos de agentes → o canvas",
    corpo: <>
      <p>Abra <b>Fluxos de agentes</b> na lateral. Os fluxos prontos <b>Tarefa → PR</b> e <b>Time de agentes</b> são um bom ponto de partida.</p>
      <ol>
        <li>Use a barra de cima para adicionar etapas: <b>Agente</b>, <b>Condição</b>, <b>Aprovação</b>, <b>Time</b> (agentes conversando), <b>Pull request</b> e <b>Fim</b>.</li>
        <li><b>Conectar</b>: arraste do quadradinho à direita de uma etapa até outra. Soltar no vazio abre o menu para criar a próxima etapa já ligada.</li>
        <li>A <b>Condição</b> tem três saídas: <span className="manual-sim">sim</span> (aprovado), <span className="manual-nao">não</span> (volta para corrigir) e <span className="manual-esgotou">esgotou</span> (passou do limite de voltas).</li>
        <li>Clique numa etapa para configurar: qual agente, prompt, modelo e esforço só nesta etapa.</li>
        <li>Para apagar uma seta, passe o mouse e clique no <b>×</b>. Para apagar uma etapa, selecione e aperte Delete.</li>
      </ol>
    </>,
  },
  {
    id: "fluxo-ia", titulo: "Criar o fluxo inteiro com IA ou JSON", imagem: "/manual/fluxo-ia.webp", legenda: "Criar fluxo com IA",
    corpo: <>
      <p>Clique em <b>Criar com IA</b> e descreva o processo — <i>“recebe um bug, um agente investiga, o dev corrige, o QA testa até 2 vezes, segurança revisa e eu aprovo antes do PR”</i>. A IA monta o fluxo completo, reaproveita seus agentes e, se faltar um papel, propõe agentes novos. Você vê o resumo antes de abrir no canvas.</p>
      <p><b>Importar JSON</b> aceita um fluxo colado (o mesmo formato salvo em <code>~/.omni/fluxos</code>); <b>Copiar JSON</b> leva o fluxo para compartilhar ou versionar.</p>
    </>,
  },
  {
    id: "executar", titulo: "Executar", imagem: "/manual/executar.webp", legenda: "O cartão de início mostra o que vai acontecer",
    corpo: <>
      <p>Com um projeto aberto, clique em <b>Executar</b>. O cartão de início mostra, etapa por etapa, o que vai acontecer; você escreve a tarefa com o critério de pronto e inicia.</p>
      <p>O OMNI cria a branch <code>omni/&lt;fluxo&gt;-&lt;id&gt;</code> num worktree separado. Se o fluxo termina num PR, ele sai no GitHub (pelo <code>gh</code>) só depois da sua aprovação.</p>
    </>,
  },
  {
    id: "acompanhar", titulo: "Acompanhar, pausar e reverter", imagem: "/manual/execucao.webp", legenda: "A execução ao vivo",
    corpo: <>
      <ul>
        <li>No canvas, a etapa que está trabalhando <b>pulsa</b>; cada uma mostra voltas, tempo e custo. O resumo à direita diz a branch, o worktree e <b>o que o agente está fazendo agora</b>.</li>
        <li><b>Pausar</b> para o agente na hora; <b>Retomar</b> continua da mesma sessão. <b>Cancelar</b> encerra.</li>
        <li>Clique numa etapa para ver o <b>histórico</b> (falas e comandos) e a saída.</li>
        <li><b>Abrir numa conversa</b> abre aquele agente numa aba, na mesma sessão e conta — peça um ajuste ou que ele <b>desfaça o que fez</b>.</li>
        <li>Etapas de <b>Aprovação</b> esperam você clicar em Aprovar ou Recusar.</li>
      </ul>
    </>,
  },
  {
    id: "dicas", titulo: "Dicas",
    corpo: <>
      <ul>
        <li>No prompt de uma etapa: <code>{"{{entrada}}"}</code> é a tarefa, <code>{"{{anterior}}"}</code> é o que veio da etapa anterior (num loop, o relatório do QA) e <code>{"{{no:plano}}"}</code> é a saída da etapa <i>plano</i>.</li>
        <li>Defina um limite no loop (2–4 voltas costuma bastar) e uma saída <i>esgotou</i>: sem limite, ele só para no botão Parar.</li>
        <li>Agentes de veredito terminam com <code>{"{\"veredito\": \"aprovado\"}"}</code>: é isso que a Condição lê.</li>
        <li>Os agentes editam arquivos e rodam comandos sem pedir confirmação, dentro do worktree e com as ferramentas do modelo. Por isso o PR passa pela sua aprovação.</li>
      </ul>
    </>,
  },
];

/** Manual dos agentes e fluxos: índice à esquerda, seção com print à direita. */
export function ManualDeAgentes({ onFechar, inicio = "visao" }: { onFechar: () => void; inicio?: string }) {
  const [atual, setAtual] = useState(inicio);
  const secao = SECOES.find((s) => s.id === atual) ?? SECOES[0];
  const indice = SECOES.indexOf(secao);
  useEffect(() => {
    const tecla = (e: KeyboardEvent) => { if (e.key === "Escape") onFechar(); };
    window.addEventListener("keydown", tecla);
    return () => window.removeEventListener("keydown", tecla);
  }, [onFechar]);

  return createPortal(
    <div className="manual-fundo" onMouseDown={(e) => { if (e.target === e.currentTarget) onFechar(); }}>
      <div className="manual" role="dialog" aria-modal="true" aria-label="Manual de agentes e fluxos">
        <nav className="manual-indice" aria-label="Seções do manual">
          <p className="manual-marca pixel-text">Manual</p>
          {SECOES.map((s, i) => (
            <button key={s.id} type="button" aria-current={s.id === secao.id ? "true" : undefined} onClick={() => setAtual(s.id)}>
              <span>{String(i + 1).padStart(2, "0")}</span>{s.titulo}
            </button>
          ))}
        </nav>
        <article className="manual-conteudo">
          <header className="manual-topo">
            <h2 className="pixel-text">{secao.titulo}</h2>
            <button type="button" className="fx-icone-botao" onClick={onFechar} aria-label="Fechar manual"><CloseIcon aria-hidden className="h-4 w-4" /></button>
          </header>
          {secao.imagem && (
            <figure className="manual-figura">
              <img src={secao.imagem} alt={secao.legenda ?? ""} loading="lazy" />
              {secao.legenda && <figcaption>{secao.legenda}</figcaption>}
            </figure>
          )}
          <div className="manual-texto">{secao.corpo}</div>
          <footer className="manual-rodape">
            <button type="button" disabled={indice === 0} onClick={() => setAtual(SECOES[indice - 1].id)}>← Anterior</button>
            <span>{indice + 1} de {SECOES.length}</span>
            <button type="button" disabled={indice === SECOES.length - 1} onClick={() => setAtual(SECOES[indice + 1].id)}>Próximo →</button>
          </footer>
        </article>
      </div>
    </div>,
    document.body,
  );
}

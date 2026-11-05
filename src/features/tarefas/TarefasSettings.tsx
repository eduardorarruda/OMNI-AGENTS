import { useEffect, useId, useState } from "react";
import { Button, Card, Field, Input } from "../../components/ui";
import {
  CONFIGURACAO_VAZIA,
  atualizarTarefas,
  lerConfiguracao,
  salvarConfiguracao,
  type ConfiguracaoDeTarefas,
  type EstadoDasTarefas,
} from "./tarefasService";

function Campo({ rotulo, valor, onChange, dica, exemplo }: { rotulo: string; valor: string; onChange: (valor: string) => void; dica?: string; exemplo?: string }) {
  const id = useId();
  return (
    <Field label={rotulo} htmlFor={id} description={dica}>
      <Input id={id} size="sm" value={valor} placeholder={exemplo} autoComplete="off" spellCheck={false} onChange={(evento) => onChange(evento.target.value)} />
    </Field>
  );
}

/** Espaço colado junto da URL ou do campo faria a API não achar nada, sem pista do motivo. */
const semEspacosNasPontas = (configuracao: ConfiguracaoDeTarefas): ConfiguracaoDeTarefas =>
  JSON.parse(JSON.stringify(configuracao), (_, valor) => (typeof valor === "string" ? valor.trim() : valor));

function resumoDoTeste(estado: EstadoDasTarefas): string {
  const quantas = estado.tarefas.length === 1 ? "1 tarefa" : `${estado.tarefas.length} tarefas`;
  return `Vieram ${quantas} (${estado.recebidas} na lista antes dos filtros).`;
}

/** Configurações → Tarefas: como ler a API de tarefas da pessoa. Nada aqui vem preenchido. */
export function TarefasSettings() {
  const [config, setConfig] = useState<ConfiguracaoDeTarefas>(CONFIGURACAO_VAZIA);
  const [chave, setChave] = useState("");
  const [temChave, setTemChave] = useState(false);
  const [ocupado, setOcupado] = useState(false);
  const [erro, setErro] = useState<string | null>(null);
  const [resultado, setResultado] = useState<{ texto: string; avisos: string[] } | null>(null);

  useEffect(() => {
    lerConfiguracao().then((lida) => { setConfig(lida.configuracao); setTemChave(lida.chave_configurada); })
      .catch((motivo) => setErro(String(motivo)));
  }, []);

  async function salvar(removerChave = false) {
    await salvarConfiguracao(semEspacosNasPontas(config), chave.trim() || null, removerChave);
    const salva = await lerConfiguracao();
    setConfig(salva.configuracao);
    setTemChave(salva.chave_configurada);
    setChave("");
  }

  async function agir(acao: () => Promise<{ texto: string; avisos: string[] }>) {
    setOcupado(true); setErro(null); setResultado(null);
    try { setResultado(await acao()); }
    catch (motivo) { setErro(String(motivo)); }
    finally { setOcupado(false); }
  }

  const aoSalvar = () => agir(async () => { await salvar(); return { texto: "Salvo.", avisos: [] }; });
  const aoRemover = () => agir(async () => { await salvar(true); return { texto: "Chave removida.", avisos: [] }; });
  // O teste é uma busca de verdade: conta no bloqueio de 60 s e já deixa o cache pronto.
  const aoTestar = () => agir(async () => {
    await salvar();
    const estado = await atualizarTarefas();
    return { texto: resumoDoTeste(estado), avisos: estado.avisos };
  });

  const campos = (nome: keyof ConfiguracaoDeTarefas["campos"]) => (valor: string) => setConfig((atual) => ({ ...atual, campos: { ...atual.campos, [nome]: valor } }));
  const filtro = (nome: "campo" | "valor") => (valor: string) => setConfig((atual) => ({ ...atual, filtro: { ...atual.filtro, [nome]: valor } }));
  const comentarios = (nome: "caminho" | "campo_tarefa" | "campo_texto") => (valor: string) => setConfig((atual) => ({ ...atual, comentarios: { ...atual.comentarios, [nome]: valor } }));
  const filtroDosComentarios = (nome: "campo" | "valor") => (valor: string) =>
    setConfig((atual) => ({ ...atual, comentarios: { ...atual.comentarios, filtro: { ...atual.comentarios.filtro, [nome]: valor } } }));
  const pessoas = (nome: keyof ConfiguracaoDeTarefas["pessoas"]) => (valor: string) => setConfig((atual) => ({ ...atual, pessoas: { ...atual.pessoas, [nome]: valor } }));
  const raiz = (nome: "url" | "cabecalho" | "caminho_lista" | "excluir_se") => (valor: string) => setConfig((atual) => ({ ...atual, [nome]: valor }));

  return (
    <div className="space-y-4">
      <p className="text-[11px] text-text-secondary">
        Mostra na aba Tarefas o que vem da sua API. O app só busca quando você pede ou quando o cache passa de 15 minutos, e nunca mais de uma vez por minuto.
      </p>

      <Card title="Conexão" aside={temChave ? <span className="text-[11px] text-text-muted">configurada</span> : undefined}>
        <Campo rotulo="URL completa da API" valor={config.url} onChange={raiz("url")} exemplo="https://api.exemplo.test/dados?tabelas=a,b" dica="Precisa ser https. Só localhost pode usar http." />
        <Campo rotulo="Nome do cabeçalho de autenticação" valor={config.cabecalho} onChange={raiz("cabecalho")} exemplo="X-Chave" />
        <ChaveDeAcesso valor={chave} onChange={setChave} temChave={temChave} desligado={ocupado} onRemover={() => void aoRemover()} />
      </Card>

      <Card title="Onde estão as tarefas">
        <Campo rotulo="Caminho da lista no JSON" valor={config.caminho_lista} onChange={raiz("caminho_lista")} exemplo="tables.tarefas" dica="Pontos entram na estrutura. Vazio quer dizer que a resposta já é a lista." />
        <div className="grid grid-cols-2 gap-3">
          <Campo rotulo="Campo do id" valor={config.campos.id} onChange={campos("id")} />
          <Campo rotulo="Campo do título" valor={config.campos.titulo} onChange={campos("titulo")} />
          <Campo rotulo="Campo da descrição" valor={config.campos.descricao} onChange={campos("descricao")} />
          <Campo rotulo="Campo do status" valor={config.campos.status} onChange={campos("status")} />
          <Campo rotulo="Campo do responsável" valor={config.campos.responsavel} onChange={campos("responsavel")} />
        </div>
      </Card>

      <Card title="Quais tarefas são suas">
        <div className="grid grid-cols-2 gap-3">
          <Campo rotulo="Filtrar pelo campo" valor={config.filtro.campo} onChange={filtro("campo")} exemplo="dono_id" />
          <Campo rotulo="Com o valor" valor={config.filtro.valor} onChange={filtro("valor")} exemplo="123" />
        </div>
        <Campo rotulo="Campos que excluem a tarefa" valor={config.excluir_se} onChange={raiz("excluir_se")} exemplo="excluido,inativo" dica="Separados por vírgula. A tarefa some quando um deles é verdadeiro." />
      </Card>

      <Card title="Comentários (opcional)">
        <Campo rotulo="Caminho da lista de comentários" valor={config.comentarios.caminho} onChange={comentarios("caminho")} exemplo="tables.comentarios" />
        <div className="grid grid-cols-2 gap-3">
          <Campo rotulo="Campo que aponta a tarefa" valor={config.comentarios.campo_tarefa} onChange={comentarios("campo_tarefa")} />
          <Campo rotulo="Campo do texto" valor={config.comentarios.campo_texto} onChange={comentarios("campo_texto")} />
          <Campo rotulo="Filtro de comentários: campo" valor={config.comentarios.filtro.campo} onChange={filtroDosComentarios("campo")} />
          <Campo rotulo="Filtro de comentários: valor" valor={config.comentarios.filtro.valor} onChange={filtroDosComentarios("valor")} />
        </div>
      </Card>

      <Card title="Nomes das pessoas (opcional)">
        <Campo rotulo="Caminho da lista de pessoas" valor={config.pessoas.caminho} onChange={pessoas("caminho")} exemplo="tables.pessoas" dica="Troca o id do responsável pelo nome." />
        <div className="grid grid-cols-2 gap-3">
          <Campo rotulo="Campo do id da pessoa" valor={config.pessoas.campo_id} onChange={pessoas("campo_id")} />
          <Campo rotulo="Campo do nome" valor={config.pessoas.campo_nome} onChange={pessoas("campo_nome")} />
        </div>
      </Card>

      {erro && <p role="alert" className="border-l-2 border-danger px-2 py-1 text-[11px] text-danger">{erro}</p>}
      {resultado && (
        <div role="status" className="text-[11px] text-text-secondary">
          <p>{resultado.texto}</p>
          {resultado.avisos.map((aviso) => <p key={aviso} className="text-text-muted">{aviso}</p>)}
        </div>
      )}
      <div className="flex gap-2">
        <Button size="sm" disabled={ocupado} onClick={() => void aoSalvar()}>Salvar</Button>
        <Button size="sm" variant="ghost" disabled={ocupado} onClick={() => void aoTestar()}>Testar</Button>
      </div>
    </div>
  );
}

function ChaveDeAcesso({ valor, onChange, temChave, desligado, onRemover }: { valor: string; onChange: (valor: string) => void; temChave: boolean; desligado: boolean; onRemover: () => void }) {
  const id = useId();
  return (
    <Field label="Chave de acesso" htmlFor={id} description={temChave ? "Deixe em branco para manter a atual. Ela nunca é mostrada de novo. Remover também limpa o nome do cabeçalho." : "Fica só neste computador, em um arquivo que só o seu usuário lê."}>
      <div className="flex gap-2">
        <Input id={id} size="sm" type="password" value={valor} autoComplete="new-password" onChange={(evento) => onChange(evento.target.value)} />
        {temChave && <Button size="sm" variant="ghost" disabled={desligado} onClick={onRemover}>Remover chave</Button>}
      </div>
    </Field>
  );
}

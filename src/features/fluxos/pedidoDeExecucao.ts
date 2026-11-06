/** "Abra esta execução": o selo global deixa o pedido aqui e a tela de Fluxos o recolhe ao abrir. */
let pedido: string | null = null;
const ouvintes = new Set<() => void>();

export function pedirExecucao(id: string) { pedido = id; ouvintes.forEach((o) => o()); }
export function consumirPedidoDeExecucao(): string | null { const id = pedido; pedido = null; return id; }
/** Avisa a tela de Fluxos quando o pedido chega com ela já aberta. */
export function aoPedirExecucao(ouvinte: () => void) { ouvintes.add(ouvinte); return () => { ouvintes.delete(ouvinte); }; }

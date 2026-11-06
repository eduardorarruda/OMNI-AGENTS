import { describe, expect, it } from "vitest";
import { novosAvisosDeFluxo } from "../mobile/notificacoes";

describe("avisos de fluxo", () => {
  it("na primeira leitura só anota o que já tinha acontecido", () => {
    const vistos = new Map<string, number>();
    expect(novosAvisosDeFluxo([{ id: "a", aviso: "falhou", aviso_seq: 2 }], vistos, true)).toEqual([]);
    expect(novosAvisosDeFluxo([{ id: "a", aviso: "falhou", aviso_seq: 2 }], vistos, false)).toEqual([]);
  });

  it("avisa uma vez quando a execução sobe o aviso_seq", () => {
    const vistos = new Map([["a", 1]]);
    const evento = { id: "a", aviso: "aprovacao", aviso_seq: 2 };
    expect(novosAvisosDeFluxo([evento], vistos, false)).toEqual([evento]);
    expect(novosAvisosDeFluxo([evento], vistos, false)).toEqual([]);
  });

  it("ignora engine antigo, sem campo de aviso", () => {
    expect(novosAvisosDeFluxo([{ id: "a" }], new Map(), false)).toEqual([]);
  });
});

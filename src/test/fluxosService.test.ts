import { expect, it } from "vitest";
import { SAIDAS, TIPOS } from "../features/fluxos/fluxosService";

it("Subfluxo aparece entre os tipos", () => {
  expect(TIPOS.map((t) => t.tipo)).toContain("subfluxo");
  expect(TIPOS.find((t) => t.tipo === "subfluxo")?.nome).toBe("Subfluxo");
  expect(SAIDAS.subfluxo).toEqual([""]);
});

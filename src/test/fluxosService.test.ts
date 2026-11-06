import { expect, it } from "vitest";
import { limiteDoCampo, SAIDAS, TIPOS } from "../features/fluxos/fluxosService";

it("Subfluxo aparece entre os tipos", () => {
  expect(TIPOS.map((t) => t.tipo)).toContain("subfluxo");
  expect(TIPOS.find((t) => t.tipo === "subfluxo")?.nome).toBe("Subfluxo");
  expect(SAIDAS.subfluxo).toEqual([""]);
});

it("campo de limite aceita qualquer número e vazio é sem limite", () => {
  expect(limiteDoCampo("50")).toBe(50);
  expect(limiteDoCampo("")).toBeUndefined();
  expect(limiteDoCampo("0")).toBe(1);
});

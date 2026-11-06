import { afterEach, describe, expect, it, vi } from "vitest";
import { enterEnvia } from "../mobile/Composer";
import { pareceArquivo } from "../mobile/api";

function ponteiro(fino: boolean) {
  vi.stubGlobal("matchMedia", (consulta: string) => ({ matches: fino && consulta.includes("fine") }));
}

describe("Enter do composer", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("envia no app do PC mesmo quando o WebKit não informa o mouse", () => {
    ponteiro(false);
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(enterEnvia()).toBe(true);
  });

  it("envia na web com mouse", () => {
    ponteiro(true);
    expect(enterEnvia()).toBe(true);
  });

  it("quebra linha no celular", () => {
    ponteiro(false);
    expect(enterEnvia()).toBe(false);
  });
});

describe("arquivo citado", () => {
  it("aceita caminho completo com espaço", () => {
    expect(pareceArquivo("~/Documentos/Vamos brincar/prompt.md")).toBe(true);
    expect(pareceArquivo("/home/eu/Vamos brincar/prompt.md:12")).toBe(true);
  });

  it("não confunde frase com arquivo", () => {
    expect(pareceArquivo("veja o prompt.md agora")).toBe(false);
  });
});

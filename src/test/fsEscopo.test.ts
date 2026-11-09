import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * Guarda do escopo de arquivos do Tauri. No Linux o `**` do plugin-fs não casa com nome que começa
 * com ponto, a menos que `requireLiteralLeadingDot` seja `false`: sem isso, abrir `.env` ou qualquer
 * coisa dentro de `.claude` na aba Arquivos dá "forbidden path". Ao liberar os ocultos, as pastas de
 * chaves continuam negadas.
 */
const raiz = join(__dirname, "..", "..", "src-tauri");
const lerJson = (arquivo: string): Record<string, unknown> => JSON.parse(readFileSync(join(raiz, arquivo), "utf8"));

interface EscopoFs { identifier: string; allow?: unknown[]; deny?: unknown[] }

function escopoFs(): EscopoFs {
  const permissoes = lerJson("capabilities/default.json").permissions as unknown[];
  const escopo = permissoes.find((p): p is EscopoFs => typeof p === "object" && p !== null && (p as EscopoFs).identifier === "fs:scope");
  if (!escopo) throw new Error("fs:scope ausente em capabilities/default.json");
  return escopo;
}

const caminhoDaRegra = (regra: unknown): string => (typeof regra === "string" ? regra : String((regra as { path?: string }).path));

describe("escopo de arquivos do app", () => {
  it("abre arquivo oculto como .env (o ** casa com nome que começa com ponto)", () => {
    const plugins = lerJson("tauri.conf.json").plugins as Record<string, Record<string, unknown>>;
    expect(plugins.fs?.requireLiteralLeadingDot).toBe(false);
  });

  it("continua negando as pastas de chaves do usuário", () => {
    const negados = (escopoFs().deny ?? []).map(caminhoDaRegra);
    for (const pasta of ["$HOME/.ssh/**", "$HOME/.gnupg/**", "$HOME/.tauri/**"]) {
      expect(negados).toContain(pasta);
    }
  });
});

import { render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { AppBackground } from "../components/backgrounds/AppBackground";
import { intervaloDaLeitura, POLL_ATIVO_MS, POLL_FALHANDO_MS, POLL_OCIOSO_LEVE_MS, POLL_OCIOSO_MS, POLL_OCULTO_LEVE_MS, POLL_OCULTO_MS } from "../features/terminal/ritmoDoTerminal";
import { definirModoLeve, ritmo } from "../lib/modoLeve";
import { DEFAULT_SETTINGS } from "../types/settings";

const normal = { falhando: false, oculto: false, ativo: false, leve: false };

describe("ritmo do terminal", () => {
  it("mantém a leitura rápida no terminal visível fora do modo leve", () => {
    expect(intervaloDaLeitura(normal)).toBe(POLL_OCIOSO_MS);
    expect(intervaloDaLeitura({ ...normal, ativo: true })).toBe(POLL_ATIVO_MS);
  });

  it("espaça a leitura do terminal oculto mesmo com atividade", () => {
    expect(intervaloDaLeitura({ ...normal, oculto: true, ativo: true })).toBe(POLL_OCULTO_MS);
  });

  it("espaça mais ainda no modo leve", () => {
    expect(intervaloDaLeitura({ ...normal, leve: true })).toBe(POLL_OCIOSO_LEVE_MS);
    expect(intervaloDaLeitura({ ...normal, leve: true, oculto: true })).toBe(POLL_OCULTO_LEVE_MS);
  });

  it("dá prioridade à espera de falha", () => {
    expect(intervaloDaLeitura({ ...normal, falhando: true, ativo: true })).toBe(POLL_FALHANDO_MS);
  });
});

describe("modo leve", () => {
  afterEach(() => definirModoLeve(false));

  it("escolhe o ritmo conforme a preferência", () => {
    expect(ritmo(1_000, 3_000)).toBe(1_000);
    definirModoLeve(true);
    expect(ritmo(1_000, 3_000)).toBe(3_000);
  });

  it("vem desligado por padrão", () => {
    expect(DEFAULT_SETTINGS.modoLeve).toBe(false);
  });

  it("não monta o fundo animado", () => {
    const { container } = render(<AppBackground settings={{ ...DEFAULT_SETTINGS, background: "gradient-waves", modoLeve: true }} />);
    expect(container.firstChild).toBeNull();
  });
});

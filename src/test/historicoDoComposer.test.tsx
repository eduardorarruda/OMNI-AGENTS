import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Composer, passoNoHistorico } from "../mobile/Composer";

function Campo({ inicial = "", historico = ["primeira", "segunda"] }: { inicial?: string; historico?: string[] }) {
  const [texto, setTexto] = useState(inicial);
  return <Composer texto={texto} onTexto={setTexto} anexos={[]} onAdicionar={vi.fn()} onRemover={vi.fn()} onEnviar={vi.fn()}
    podeEnviar={false} ocupado={false} placeholder="Mensagem" historico={historico} />;
}

const campo = () => screen.getByPlaceholderText("Mensagem") as HTMLTextAreaElement;
const seta = (tecla: "ArrowUp" | "ArrowDown") => fireEvent.keyDown(campo(), { key: tecla });

describe("Histórico no campo de mensagem", () => {
  it("calcula o passo", () => {
    expect(passoNoHistorico(2, null, -1)).toBe(1);
    expect(passoNoHistorico(2, 1, -1)).toBe(0);
    expect(passoNoHistorico(2, 0, -1)).toBe(0);
    expect(passoNoHistorico(2, 0, 1)).toBe(1);
    expect(passoNoHistorico(2, 1, 1)).toBeNull();
    expect(passoNoHistorico(2, null, 1)).toBeUndefined();
    expect(passoNoHistorico(0, null, -1)).toBeUndefined();
  });

  it("seta para cima traz a última, depois a anterior; para baixo volta ao campo vazio", () => {
    render(<Campo />);
    seta("ArrowUp");
    expect(campo().value).toBe("segunda");
    seta("ArrowUp");
    expect(campo().value).toBe("primeira");
    seta("ArrowUp");
    expect(campo().value).toBe("primeira");
    seta("ArrowDown");
    expect(campo().value).toBe("segunda");
    seta("ArrowDown");
    expect(campo().value).toBe("");
  });

  it("com texto digitado a seta continua sendo do cursor", () => {
    render(<Campo inicial="estou digitando" />);
    seta("ArrowUp");
    expect(campo().value).toBe("estou digitando");
  });

  it("editar a mensagem trazida encerra a navegação", () => {
    render(<Campo />);
    seta("ArrowUp");
    fireEvent.change(campo(), { target: { value: "segunda editada" } });
    seta("ArrowUp");
    expect(campo().value).toBe("segunda editada");
  });
});

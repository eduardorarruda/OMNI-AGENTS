import { useState } from "react";
import { CheckIcon, PackageIcon } from "../components/ui/PixelIcon";
import { pedirInstalacao, useComoInstalar } from "./instalarApp";

/** "Instalar no celular": vira um ícone na tela inicial, abre sem a barra do navegador. */
export function InstalarApp({ pagina = "celular" }: { pagina?: "celular" | "computador" }) {
  const modo = useComoInstalar();
  const [recusou, setRecusou] = useState(false);
  const rotulo = pagina === "celular" ? "Instalar no celular" : "Instalar como app";

  async function instalar() {
    setRecusou(!(await pedirInstalacao()));
  }

  return (
    <section className="m-ajustes-grupo glass" aria-labelledby="ajustes-instalar">
      <header className="m-ajustes-cabeca">
        <h3 id="ajustes-instalar" className="m-ajustes-titulo"><PackageIcon aria-hidden className="h-3.5 w-3.5" />Instalar como app</h3>
        {modo === "instalado" && <span className="m-ajustes-selo" data-tom="ok">instalado</span>}
      </header>
      {modo === "instalado" && <p className="m-ajustes-nota"><CheckIcon aria-hidden className="mr-1 inline h-3.5 w-3.5" />Você já está usando o OMNI como app.</p>}
      {modo === "botao" && <>
        <button type="button" className="m-ajustes-botao" onClick={() => void instalar()}>
          <PackageIcon aria-hidden className="h-4 w-4" />{rotulo}
        </button>
        <p className="m-ajustes-nota">Fica um ícone na tela inicial e abre direto, sem a barra do navegador.</p>
        {recusou && <p className="m-ajustes-nota">Sem problema: o botão volta quando o navegador deixar pedir de novo.</p>}
      </>}
      {modo === "iphone" && <ol className="m-ajustes-nota list-decimal space-y-1 pl-4">
        <li>Abra este link no <strong>Safari</strong>.</li>
        <li>Toque em <strong>Compartilhar</strong> (o quadrado com a seta para cima).</li>
        <li>Toque em <strong>Adicionar à Tela de Início</strong>.</li>
        <li>Na primeira vez que abrir pelo ícone, digite o código do Authy.</li>
      </ol>}
      {modo === "menu" && <p className="m-ajustes-nota">
        No menu do navegador (<strong>⋮</strong>), toque em <strong>Instalar app</strong> ou <strong>Adicionar à tela inicial</strong>.
      </p>}
      {modo === "sem-https" && <p className="m-ajustes-nota">
        Para instalar, abra pelo endereço com <strong>https</strong>: no PC, em Configurações → Acesso remoto, use o
        <strong> link seguro</strong> e leia o QR de novo.
      </p>}
    </section>
  );
}

import { useEffect, useLayoutEffect, useRef } from "react";
import { CloseIcon, EditIcon, FolderIcon, ImageIcon } from "./ui/PixelIcon";
import type { PixelIconComponent } from "../features/files/fileIcons";

export interface MenuDoProjetoAberto {
  projectId: string;
  nome: string;
  x: number;
  y: number;
}

interface MenuDoProjetoProps {
  menu: MenuDoProjetoAberto;
  onFechar: () => void;
  onRenomear?: () => void;
  onTrocarIcone?: () => void;
  onFecharProjeto: () => void;
}

/** Menu do botão direito na linha do projeto da lateral. Mesmo visual do menu da árvore de arquivos. */
export function MenuDoProjeto({ menu, onFechar, onRenomear, onTrocarIcone, onFecharProjeto }: MenuDoProjetoProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const fora = (evento: PointerEvent) => { if (!ref.current?.contains(evento.target as Node)) onFechar(); };
    const tecla = (evento: KeyboardEvent) => { if (evento.key === "Escape") onFechar(); };
    document.addEventListener("pointerdown", fora);
    document.addEventListener("keydown", tecla);
    return () => {
      document.removeEventListener("pointerdown", fora);
      document.removeEventListener("keydown", tecla);
    };
  }, [onFechar]);

  // Abre onde clicou, sem vazar da janela.
  useLayoutEffect(() => {
    const caixa = ref.current;
    if (!caixa) return;
    const { width, height } = caixa.getBoundingClientRect();
    caixa.style.position = "fixed";
    caixa.style.left = `${Math.max(8, Math.min(menu.x, window.innerWidth - width - 8))}px`;
    caixa.style.top = `${Math.max(8, menu.y + height > window.innerHeight - 8 ? menu.y - height : menu.y)}px`;
    caixa.querySelector<HTMLElement>("[role=menuitem]")?.focus();
  }, [menu]);

  function navegar(evento: React.KeyboardEvent) {
    if (evento.key !== "ArrowDown" && evento.key !== "ArrowUp") return;
    evento.preventDefault();
    const itens = [...(ref.current?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
    const atual = itens.indexOf(document.activeElement as HTMLElement);
    itens[(atual + (evento.key === "ArrowDown" ? 1 : -1) + itens.length) % itens.length]?.focus();
  }

  const escolher = (acao: () => void) => () => { onFechar(); acao(); };

  return (
    <div ref={ref} role="menu" aria-label={`Ações do projeto ${menu.nome}`} onKeyDown={navegar} className="popover arvore-menu z-50">
      <div className="arvore-menu-cabeca">
        <FolderIcon className="h-3.5 w-3.5 shrink-0" aria-hidden />
        <span className="truncate">{menu.nome}</span>
      </div>
      {onRenomear && <Item icone={EditIcon} rotulo="Renomear" atalho="F2" onClick={escolher(onRenomear)} />}
      {onTrocarIcone && <Item icone={ImageIcon} rotulo="Trocar ícone" onClick={escolher(onTrocarIcone)} />}
      <hr role="separator" className="arvore-menu-linha" />
      <Item icone={CloseIcon} rotulo="Fechar projeto" onClick={escolher(onFecharProjeto)} perigo />
    </div>
  );
}

function Item({ icone: Icone, rotulo, atalho, onClick, perigo }: { icone: PixelIconComponent; rotulo: string; atalho?: string; onClick: () => void; perigo?: boolean }) {
  return (
    <button type="button" role="menuitem" onClick={onClick} aria-keyshortcuts={atalho} className={`arvore-menu-item${perigo ? " is-perigo" : ""}`}>
      <span className="arvore-menu-icone"><Icone className="h-3.5 w-3.5" aria-hidden /></span>
      <span className="flex-1 truncate">{rotulo}</span>
      {atalho && <kbd aria-hidden className="arvore-menu-atalho">{atalho}</kbd>}
    </button>
  );
}

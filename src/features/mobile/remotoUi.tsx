import type { ButtonHTMLAttributes } from "react";
import { useMemo } from "react";
import QRCode from "qrcode";
import { OmniLogo } from "../../components/ui/OmniLogo";
import type { LinkIcon } from "../../components/ui/PixelIcon";

/** Caminho SVG de um quadrado de lado `l` com cantos de raio `r`. */
const quadrado = (x: number, y: number, l: number, r: number) =>
  `M${x + r} ${y}h${l - 2 * r}a${r} ${r} 0 0 1 ${r} ${r}v${l - 2 * r}a${r} ${r} 0 0 1-${r} ${r}h-${l - 2 * r}a${r} ${r} 0 0 1-${r}-${r}v-${l - 2 * r}a${r} ${r} 0 0 1 ${r}-${r}z`;

/** QR desenhado em SVG com a cor do app: módulos em pontos, "olhos" de canto levemente arredondado (raio maior que esse
 *  e o leitor do OpenCV já não acha o QR), moldura em
 *  degradê com linha de leitura e a logo no centro. O miolo fica sem módulos de propósito: o QR sai com
 *  correção "H" (30%) para isso. Fundo branco porque a câmera precisa do contraste. */
export function QrPersonalizado({ texto, alt, tamanho, cor }: { texto: string; alt: string; tamanho: number; cor: string }) {
  const desenho = useMemo(() => {
    try {
      const { size: n, data } = QRCode.create(texto, { errorCorrectionLevel: "H" }).modules;
      const olho = (x: number, y: number) => (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7);
      const miolo = Math.ceil(n * 0.22) | 1, m0 = (n - miolo) / 2;
      const pontos: string[] = [];
      for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
        if (!data[y * n + x] || olho(x, y) || (x >= m0 && x < m0 + miolo && y >= m0 && y < m0 + miolo)) continue;
        pontos.push(`M${x + 0.5} ${y + 0.08}a.42 .42 0 1 1 0 .84a.42 .42 0 1 1 0-.84`);
      }
      return { n, d: pontos.join(""), olhos: [[0, 0], [n - 7, 0], [0, n - 7]] };
    } catch { return null; }
  }, [texto]);
  if (!desenho) return null;
  const { n, d, olhos } = desenho;
  return <div className="remoto-qr shrink-0">
    <div className="remoto-qr-papel">
      <svg role="img" aria-label={alt} width={tamanho} height={tamanho} viewBox={`-1 -1 ${n + 2} ${n + 2}`} className="block">
        <path d={d} fill={cor} />
        {olhos.map(([x, y]) => <g key={`${x}-${y}`} fill={cor}>
          <path fillRule="evenodd" d={quadrado(x, y, 7, 0.6) + quadrado(x + 1, y + 1, 5, 0.3)} />
          <rect x={x + 2} y={y + 2} width={3} height={3} rx={0.8} />
        </g>)}
      </svg>
      <span aria-hidden className="remoto-qr-logo"><OmniLogo size={Math.round(tamanho * 0.15)} /></span>
      <span aria-hidden className="remoto-qr-leitura" />
    </div>
  </div>;
}

/** Botão desta tela: ícone + texto, contorno com brilho; `forte` é a ação principal do passo. */
export function Acao({ icone: Icone, forte, children, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { icone?: typeof LinkIcon; forte?: boolean }) {
  return <button type="button" className={`remoto-acao${forte ? " is-forte" : ""}`} {...rest}>
    {Icone && <Icone size={14} aria-hidden />}{children}
  </button>;
}


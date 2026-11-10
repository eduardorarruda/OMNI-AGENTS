/** Manifest do app instalável (PWA). O build do celular (`vite.mobile.config.ts`) o grava como
 *  `manifest.webmanifest`; fica aqui, fora do config, para o teste conferir o que o Chrome exige. */

/** Cor de fundo do app — a mesma do terminal do desktop. Usada na barra do celular e no splash. */
export const FUNDO = "#0e0e14";

export const MANIFEST = {
  // Identidade do app instalado; sem ela o Chrome usa o `start_url`.
  id: "/",
  name: "OMNI AGENTS",
  short_name: "OMNI",
  description: "Suas conversas com os agentes do PC, no celular.",
  lang: "pt-BR",
  start_url: "/",
  scope: "/",
  display: "standalone",
  background_color: FUNDO,
  theme_color: FUNDO,
  icons: [
    { src: "/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
    { src: "/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
  ],
};

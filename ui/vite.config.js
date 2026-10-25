import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { viteSingleFile } from 'vite-plugin-singlefile';

// L'interface est compilée en UN seul fichier HTML (JS, CSS et police inclus),
// que le binaire Rust embarque : rien à installer ni à servir à côté, et ça marche hors ligne.
export default defineConfig({
  plugins: [svelte(), viteSingleFile()],
  build: {
    outDir: '../static',
    emptyOutDir: false,
    assetsInlineLimit: 100_000_000,
  },
  server: {
    // `npm run dev` : l'interface se recharge à chaud et parle au Heim lancé sur le port 8090.
    proxy: {
      '/api': { target: 'http://localhost:8090', ws: true },
      '^/(sw\\.js|manifest\\.webmanifest|icon-\\d+\\.png|apple-touch-icon\\.png)$': 'http://localhost:8090',
    },
  },
});

import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri serves the built files from dist/ and uses this dev server in `npm run desktop`
export default defineConfig({
	plugins: [svelte()],
	clearScreen: false,
	server: { port: 5175, strictPort: true },
});

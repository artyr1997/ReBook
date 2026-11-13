import '@fontsource/kurale/400.css';
import '@fontsource/noto-sans-jp/300.css';
import '@fontsource/noto-sans-jp/400.css';
import '@fontsource/noto-sans-jp/500.css';
import './app.css';
import { invoke } from '@tauri-apps/api/core';
import { mount } from 'svelte';
import App from './App.svelte';

const app = mount(App, { target: document.getElementById('app')! });

// drawn, with its fonts (or not later than 300 ms): the window fades in (it starts hidden)
Promise.race([document.fonts.ready, new Promise((done) => setTimeout(done, 300))]).then(() => invoke('window_ready'));

// ⌘N: another window (src-tauri/src/lib.rs, new_window), before the app's own keys
window.addEventListener(
	'keydown',
	(e) => {
		if (e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && e.code === 'KeyN') {
			e.preventDefault();
			e.stopImmediatePropagation();
			invoke('new_window');
		}
	},
	true,
);

export default app;

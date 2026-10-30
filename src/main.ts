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

export default app;

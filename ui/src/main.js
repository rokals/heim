import '@fontsource/familjen-grotesk/latin-400.css';
import '@fontsource/familjen-grotesk/latin-600.css';
import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { connect } from './lib/store.svelte.js';
import { registerWorker } from './lib/push.js';

connect();
// Le service worker reçoit les notifications push (seulement en HTTPS).
registerWorker();
mount(App, { target: document.getElementById('app') });

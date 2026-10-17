import { mount } from 'svelte';
import App from './App.svelte';
import './styles/app.css';

const target = document.getElementById('app');
if (!target) throw new Error('找不到应用挂载点');
mount(App, { target });

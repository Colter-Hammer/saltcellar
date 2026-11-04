import { VueQueryPlugin } from '@tanstack/vue-query'
import { createApp } from 'vue'
import { createWebHistory } from 'vue-router'
import App from './App.vue'
import { createAppRouter } from './router'
import './styles/main.css'
import './theme/palettes'

createApp(App).use(createAppRouter(createWebHistory())).use(VueQueryPlugin).mount('#app')

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import './assets/base.css'

import App from './App.vue'
import router from './router'
import { initTheme } from './composables/useTheme'
import { initCoreClient } from './core/ipc'

// Тема ставится до первого кадра, чтобы не мигнуть светлым на тёмной системе.
initTheme()

// Клиент ядра поднимается ДО монтирования: единственная точка, где решается,
// говорим мы с реальным Rust-ядром или с мок-ядром (см. src/core/ipc.ts).
void initCoreClient().then(() => {
  const app = createApp(App)

  app.use(createPinia())
  app.use(router)

  app.mount('#app')
})

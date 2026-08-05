import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import { initCoreClient } from './core/ipc'

// Клиент ядра поднимается ДО монтирования: единственная точка, где решается,
// говорим мы с реальным Rust-ядром или с мок-ядром (см. src/core/ipc.ts).
void initCoreClient().then(() => {
  const app = createApp(App)

  app.use(createPinia())
  app.use(router)

  app.mount('#app')
})

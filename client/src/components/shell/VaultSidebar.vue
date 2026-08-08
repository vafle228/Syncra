<script setup lang="ts">
import { useRouter } from 'vue-router'

import SectionSidebar from '@/components/sections/SectionSidebar.vue'
import { useDevicesStore } from '@/stores/useDevicesStore'
import { useVaultStore } from '@/stores/useVaultStore'

import SidebarSync from './SidebarSync.vue'

/**
 * Левая колонка окна: секции, навигация, состояние обмена, замок (F13).
 *
 * Сайдбар не исчезает ни на одном экране открытого хранилища — в этом весь смысл
 * оболочки. До F13 «Устройства» и «Настройки» были отдельными страницами со
 * своими шапками и ссылкой «← К паролям»; теперь это строки одного окна, и
 * возвращаться неоткуда, потому что никуда не уходили.
 *
 * Кнопка замка стоит внизу и намеренно не выделена: запирать хранилище — это
 * нормальное завершение работы, а не опасное действие.
 */

const router = useRouter()
const vault = useVaultStore()
const devices = useDevicesStore()

async function lock(): Promise<void> {
  try {
    await vault.lock()
    await router.push({ name: 'unlock' })
  } catch {
    /* сообщение уже в vault.error */
  }
}
</script>

<template>
  <aside class="vault-sidebar" data-test="sidebar">
    <SectionSidebar />

    <hr class="vault-sidebar__rule" />

    <nav class="vault-sidebar__nav" aria-label="Разделы">
      <RouterLink class="vault-sidebar__link" :to="{ name: 'devices' }">
        <span class="vault-sidebar__glyph vault-sidebar__glyph--square" aria-hidden="true" />
        <span class="vault-sidebar__label">Устройства</span>
        <span class="vault-sidebar__count">{{ devices.active.length }}</span>
      </RouterLink>

      <RouterLink class="vault-sidebar__link" :to="{ name: 'settings' }">
        <span class="vault-sidebar__glyph vault-sidebar__glyph--round" aria-hidden="true" />
        <span class="vault-sidebar__label">Настройки</span>
      </RouterLink>
    </nav>

    <div class="vault-sidebar__foot">
      <SidebarSync />

      <button
        type="button"
        class="vault-sidebar__lock"
        :disabled="vault.busy"
        data-test="lock"
        @click="lock"
      >
        Запереть хранилище
      </button>
    </div>
  </aside>
</template>

<style scoped>
.vault-sidebar {
  flex: none;
  width: var(--sy-sidebar-width);
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: auto;
  border-right: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.vault-sidebar__rule {
  flex: none;
  height: 1px;
  margin: 0 var(--sy-space-5);
  border: none;
  background: var(--sy-border);
}

.vault-sidebar__nav {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: var(--sy-space-4) var(--sy-space-5) 0;
}

.vault-sidebar__link {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  height: 34px;
  padding: 0 var(--sy-space-4);
  border: 1px solid transparent;
  border-radius: var(--sy-radius-sm);
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  text-decoration: none;
  transition:
    background var(--sy-transition),
    color var(--sy-transition);
}

.vault-sidebar__link:hover {
  background: var(--sy-surface);
  color: var(--sy-text);
}

.vault-sidebar__link:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

/*
 * Активность рисует роутер: `aria-current="page"` он проставляет сам, и
 * состояние «где я» остаётся одной правдой — адресом, а не копией в состоянии.
 */
.vault-sidebar__link.router-link-active {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-text);
  font-weight: var(--sy-weight-medium);
}

.vault-sidebar__glyph {
  flex: none;
  width: 13px;
  height: 13px;
  border: 1.5px solid var(--sy-text-3);
}

.vault-sidebar__glyph--square {
  border-radius: 3px;
}

.vault-sidebar__glyph--round {
  border-radius: 50%;
}

.vault-sidebar__link.router-link-active .vault-sidebar__glyph {
  border-color: var(--sy-accent);
}

.vault-sidebar__label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.vault-sidebar__count {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-3);
}

.vault-sidebar__link.router-link-active .vault-sidebar__count {
  color: var(--sy-accent);
}

/* Подвал прижат вниз: между навигацией и им ровно столько пустоты, сколько есть. */
.vault-sidebar__foot {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5);
}

.vault-sidebar__lock {
  height: 32px;
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: transparent;
  color: var(--sy-text-2);
  font-family: inherit;
  font-size: 12.5px;
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.vault-sidebar__lock:hover:not(:disabled) {
  border-color: var(--sy-border-strong);
  color: var(--sy-text);
}

.vault-sidebar__lock:disabled {
  cursor: progress;
  opacity: 0.6;
}

.vault-sidebar__lock:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

/* Узкое окно: сайдбар ложится полосой над остальными панелями. */
@media (max-width: 1080px) {
  .vault-sidebar {
    width: 100%;
    border-right: none;
    border-bottom: 1px solid var(--sy-border);
  }
}
</style>

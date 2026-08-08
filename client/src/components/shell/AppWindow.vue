<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRouter } from 'vue-router'

import { SyThemeToggle, SyToast } from '@/components/ui'
import { useVaultStore } from '@/stores/useVaultStore'

import WindowControls from './WindowControls.vue'

/**
 * Шасси окна (F13): рамка, полоса заголовка 36px, кнопки окна, хост тостов.
 *
 * Это самый внешний слой приложения — он есть и на экране блокировки, поэтому
 * живёт выше `VaultShell` с его сайдбаром. Всё, что всплывает внутри окна
 * (тосты, модалки), лежит здесь: `position: relative` на шасси делает окно
 * системой координат для них, как в прототипе.
 *
 * Здесь же единственный на всё приложение `SyThemeToggle`. До F13 их было шесть
 * — по одному в шапке каждого экрана; переключатель темы это свойство окна, а не
 * страницы, и шесть его копий были следствием того, что окна не существовало.
 */

const router = useRouter()
const vault = useVaultStore()

/**
 * Заголовок окна из прототипа: он же говорит, заперто ли хранилище.
 *
 * «Заперто» неверно, пока хранилища ещё нет: запирать нечего, человек проходит
 * знакомство. Поэтому у онбординга свой заголовок — тот, что стоял в его
 * собственной полосе до появления окна.
 */
const title = computed(() => {
  if (vault.status === 'unlocked') return 'Syncra — Хранилище'
  if (vault.status === 'uninitialized') return 'Syncra — знакомство'
  return 'Syncra — заперто'
})

/*
 * Замок защёлкнулся сам — уводим на экран входа.
 *
 * Раньше это делал только обработчик кнопки «Заблокировать», поэтому
 * автоблокировка по бездействию (`locked` с `reason: 'timeout'`) оставляла
 * открытый экран с метаданными записей до следующей навигации. Хранитель
 * закрывал переходы, но никто их не начинал.
 *
 * Место именно здесь: окно живёт всё время работы приложения, а экраны
 * сменяются. Наблюдатель в экране пропустил бы событие, случившееся между двумя
 * экранами.
 */
watch(
  () => vault.status,
  (status, previous) => {
    if (status === 'locked' && previous === 'unlocked') {
      void router.push({ name: 'unlock' })
    }
  },
)
</script>

<template>
  <div class="app-window">
    <!--
      Полоса заголовка. `data-tauri-drag-region` даёт тянуть окно за неё, когда
      системной рамки нет; в браузере атрибут просто ничего не значит.
    -->
    <header class="app-window__titlebar" data-tauri-drag-region data-test="titlebar">
      <div class="app-window__brand">
        <span class="app-window__mark" aria-hidden="true"><span /></span>
        <span class="app-window__title">{{ title }}</span>
      </div>

      <div class="app-window__controls">
        <SyThemeToggle />
        <WindowControls />
      </div>
    </header>

    <div class="app-window__body">
      <slot />
    </div>

    <SyToast />
  </div>
</template>

<style scoped>
.app-window {
  position: relative;
  display: flex;
  flex-direction: column;
  /*
   * Окно занимает вьюпорт целиком. Фиксированные 1440×884 из прототипа — это
   * оснастка макета, а не размер приложения: настоящее окно тянут за угол.
   */
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: var(--sy-bg-1);
  color: var(--sy-text);
}

.app-window__titlebar {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  height: var(--sy-titlebar-height);
  padding: 0 var(--sy-space-5) 0 14px;
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.app-window__brand {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}

/* Логотип: квадрат в квадрате. Ни одной внешней картинки — см. `CLAUDE.md`. */
.app-window__mark {
  flex: none;
  display: grid;
  place-items: center;
  width: 15px;
  height: 15px;
  border: 1.5px solid var(--sy-accent);
  border-radius: 5px;
}

.app-window__mark > span {
  width: 5px;
  height: 5px;
  border-radius: 2px;
  background: var(--sy-accent);
}

.app-window__title {
  overflow: hidden;
  font-size: 12px;
  color: var(--sy-text-2);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.app-window__controls {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

/*
 * `min-height: 0` обязателен: без него длинная запись растянула бы окно и
 * внутренние скроллеры перестали бы работать.
 */
.app-window__body {
  flex: 1;
  display: flex;
  min-height: 0;
}

/*
 * `min-height: 0` и на детях: иначе содержимое экрана продавливает окно вместо
 * того, чтобы прокручиваться внутри своей панели.
 */
.app-window__body > * {
  flex: 1;
  min-width: 0;
  min-height: 0;
}
</style>

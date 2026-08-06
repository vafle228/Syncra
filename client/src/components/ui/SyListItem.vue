<script setup lang="ts">
import { computed } from 'vue'

import { iconHue, iconInitials } from './localIcon'

/**
 * Строка списка (F2, вариант A из макета — две строки, высота 56 px).
 *
 * ЗАКОН №1: пароль в строке не показывается НИКОГДА (§3.1). Компонент принимает
 * только метаданные и намеренно не имеет пропа под секретное значение.
 */

const props = withDefaults(
  defineProps<{
    /** Имя сервиса — первая строка. */
    title: string
    /** Логин — вторая строка. */
    subtitle?: string
    /** Метка аккаунта («личный» / «рабочий»), отличает аккаунты одного сервиса. */
    badge?: string | null
    /**
     * Строка, из которой считается тон плитки. Обычно домен: он стабильнее
     * имени сервиса. По умолчанию — `title`.
     */
    seed?: string
    selected?: boolean
    /** Отметка состояния синхронизации записи. */
    status?: 'none' | 'pending' | 'conflict'
  }>(),
  { badge: null, selected: false, status: 'none' },
)

const hue = computed(() => iconHue(props.seed ?? props.title))
const initials = computed(() => iconInitials(props.title))

const statusLabel = computed(() => {
  if (props.status === 'pending') return 'ждёт синхронизации'
  if (props.status === 'conflict') return 'конфликт версий'
  return null
})
</script>

<template>
  <div class="sy-list-item" :class="{ 'sy-list-item--selected': selected }">
    <span
      class="sy-list-item__icon"
      :style="{
        background: `oklch(var(--sy-icon-bg) ${hue})`,
        borderColor: `oklch(var(--sy-icon-border) ${hue})`,
        color: `oklch(var(--sy-icon-ink) ${hue})`,
      }"
      aria-hidden="true"
      >{{ initials }}</span
    >

    <span class="sy-list-item__text">
      <span class="sy-list-item__title">{{ title }}</span>
      <span v-if="subtitle" class="sy-list-item__subtitle">{{ subtitle }}</span>
    </span>

    <span v-if="badge" class="sy-list-item__badge">{{ badge }}</span>

    <span
      v-if="statusLabel"
      class="sy-list-item__status"
      :class="`sy-list-item__status--${status}`"
    >
      <span class="sy-list-item__status-dot" aria-hidden="true" />
      {{ statusLabel }}
    </span>

    <span v-if="$slots.actions" class="sy-list-item__actions">
      <slot name="actions" />
    </span>
  </div>
</template>

<style scoped>
.sy-list-item {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  height: var(--sy-row-height);
  padding: 0 var(--sy-space-5) 0 var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  transition: background var(--sy-transition);
}

.sy-list-item:hover {
  background: var(--sy-surface);
}

.sy-list-item--selected {
  padding-left: 13px;
  border-left: 3px solid var(--sy-accent);
  background: var(--sy-accent-quiet);
}

.sy-list-item__icon {
  flex: none;
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border: 1px solid transparent;
  border-radius: 9px;
  font-family: var(--sy-font-mono);
  font-size: 12.5px;
  font-weight: var(--sy-weight-semibold);
}

.sy-list-item__text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.sy-list-item__title {
  font-size: 14.5px;
  font-weight: var(--sy-weight-medium);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sy-list-item__subtitle {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sy-list-item__badge {
  flex: none;
  padding: 2px 7px;
  border: 1px solid var(--sy-border-strong);
  border-radius: 5px;
  font-family: var(--sy-font-mono);
  font-size: 10px;
  color: var(--sy-text-2);
}

.sy-list-item__status {
  flex: none;
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 11.5px;
}

.sy-list-item__status--pending {
  color: var(--sy-warn);
}

.sy-list-item__status--conflict {
  color: var(--sy-danger);
}

.sy-list-item__status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}

.sy-list-item__actions {
  flex: none;
  display: flex;
  gap: var(--sy-space-2);
}
</style>

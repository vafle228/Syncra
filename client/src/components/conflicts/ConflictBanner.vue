<script setup lang="ts">
/**
 * Полоса «эту запись правили в двух местах» в карточке (F11, § макета «Две
 * версии одной записи»).
 *
 * Она не модальная и ничего не закрывает: конфликт ждёт решения столько,
 * сколько нужно, и мешать человеку смотреть свою же запись он не должен. Но
 * стоит она НАД карточкой — до того, как он поверит, что видит единственную
 * версию пароля.
 */

defineProps<{
  /** Когда версии разошлись — «правили, пока устройства не виделись». */
  deviceName: string
}>()

const emit = defineEmits<{ open: [] }>()
</script>

<template>
  <div class="conflict-banner" role="status">
    <span class="conflict-banner__dot" aria-hidden="true" />
    <div class="conflict-banner__text">
      <span class="conflict-banner__title">Эту запись правили на двух устройствах</span>
      <span class="conflict-banner__note">
        Ничего не потеряно: обе версии целы, выбрать нужно одну. Вторая правка пришла с «{{
          deviceName
        }}».
      </span>
    </div>
    <button type="button" class="conflict-banner__action" @click="emit('open')">
      Показать обе версии
    </button>
  </div>
</template>

<style scoped>
.conflict-banner {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius);
  background: var(--sy-danger-quiet);
}

.conflict-banner__dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--sy-danger);
}

.conflict-banner__text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.conflict-banner__title {
  font-size: 13.5px;
  font-weight: var(--sy-weight-semibold);
}

.conflict-banner__note {
  font-size: var(--sy-text-small);
  line-height: 1.45;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.conflict-banner__action {
  flex: none;
  height: 34px;
  padding: 0 var(--sy-space-6);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius-sm);
  background: transparent;
  color: var(--sy-danger);
  font-family: inherit;
  font-size: 13px;
  font-weight: var(--sy-weight-semibold);
  cursor: pointer;
  transition:
    background var(--sy-transition),
    color var(--sy-transition);
}

.conflict-banner__action:hover {
  background: var(--sy-danger);
  color: var(--sy-danger-fg);
}

.conflict-banner__action:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}
</style>

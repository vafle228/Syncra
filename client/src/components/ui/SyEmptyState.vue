<script setup lang="ts">
/**
 * Пустое состояние (F2).
 *
 * Тон — спокойная уверенность: объясняем, что делать дальше, и заодно
 * проговариваем, что данные никуда не уходят.
 */

defineProps<{
  title: string
  description?: string
}>()
</script>

<template>
  <div class="sy-empty">
    <div class="sy-empty__mark" aria-hidden="true">
      <span class="sy-empty__mark-inner" />
    </div>

    <p class="sy-empty__title">{{ title }}</p>
    <p v-if="description" class="sy-empty__description">{{ description }}</p>

    <div v-if="$slots.actions" class="sy-empty__actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<style scoped>
.sy-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sy-space-5);
  padding: var(--sy-space-9) var(--sy-space-8);
  text-align: center;
}

.sy-empty__mark {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius-lg);
}

.sy-empty__mark-inner {
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--sy-text-3);
  border-radius: var(--sy-space-1);
}

.sy-empty__title {
  font-size: var(--sy-text-body-strong);
  line-height: var(--sy-text-body-strong-lh);
  font-weight: var(--sy-weight-semibold);
}

.sy-empty__description {
  max-width: 330px;
  font-size: 13px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.sy-empty__actions {
  display: flex;
  gap: var(--sy-space-4);
  padding-top: var(--sy-space-1);
}
</style>

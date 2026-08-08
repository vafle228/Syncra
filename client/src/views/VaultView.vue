<script setup lang="ts">
import { onBeforeUnmount } from 'vue'

import RecordCard from '@/components/records/RecordCard.vue'
import RecordForm from '@/components/records/RecordForm.vue'
import { SyButton, SyEmptyState } from '@/components/ui'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useVaultUiStore } from '@/stores/useVaultUiStore'

/**
 * Правая панель главного экрана: карточка записи, форма или пустое состояние
 * (F4, F5, §3.1).
 *
 * Запись намеренно НЕ выбирается автоматически: открывать чью-то карточку без
 * спроса — не то, чего ждёшь от менеджера паролей на общем столе.
 *
 * ЗАКОН №1 держится на `key`. Карточка и форма пересоздаются при смене записи,
 * а вместе с ними умирает всё открытое: показанный секрет живёт в области
 * видимости компонента (`useRecordSecrets`), и пересоздание — это и есть его
 * гарантированное стирание. Менять эти ключи нельзя, не сломав обещание.
 */

const list = useRecordsStore()
const ui = useVaultUiStore()

/**
 * Уход с `/` закрывает редактор: вернувшись из настроек, человек ожидает список,
 * а не забытый наполовину заполненный черновик. Сам черновик умирает вместе с
 * `RecordForm` — стор его не хранит.
 */
onBeforeUnmount(() => {
  ui.closeEditor()
})
</script>

<template>
  <div class="vault-view" data-test="right-pane">
    <RecordForm
      v-if="ui.editor === 'create'"
      key="create"
      @saved="ui.closeEditor()"
      @cancel="ui.closeEditor()"
    />

    <RecordForm
      v-else-if="ui.editor === 'edit' && list.selected"
      :key="`edit:${list.selected.record_id}`"
      :record="list.selected"
      @saved="ui.closeEditor()"
      @cancel="ui.closeEditor()"
    />

    <RecordCard
      v-else-if="list.selected"
      :key="list.selected.record_id"
      :record="list.selected"
      @edit="ui.startEdit()"
    />

    <SyEmptyState
      v-else
      class="vault-view__empty"
      title="Запись не выбрана"
      description="Выберите строку слева, чтобы посмотреть её. Пароль не появится на экране сам — только по нажатию."
    >
      <template #actions>
        <SyButton size="sm" @click="ui.startCreate()">Новая запись</SyButton>
      </template>
    </SyEmptyState>
  </div>
</template>

<style scoped>
.vault-view {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: auto;
}

.vault-view > * {
  flex: 1;
  min-height: 0;
}

.vault-view__empty {
  align-self: center;
  justify-content: center;
  flex: 1;
}
</style>

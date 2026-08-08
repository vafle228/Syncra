<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'

import RecordList from '@/components/records/RecordList.vue'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useDevicesStore } from '@/stores/useDevicesStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useSecurityStore } from '@/stores/useSecurityStore'
import { useSyncStore } from '@/stores/useSyncStore'

import VaultSidebar from './VaultSidebar.vue'

/**
 * Оболочка открытого хранилища (F13): сайдбар, список и правая панель.
 *
 * Родительский роут всех экранов за замком. Отсюда следуют два свойства, ради
 * которых он и заведён:
 *
 *  1. Данные сеанса поднимаются ОДИН раз на вход в хранилище, а не на каждый
 *     заход на `/`. До F13 это висело на главном экране, и возврат из настроек
 *     перезапускал загрузку списка.
 *  2. Сайдбар не размонтируется при переходах между экранами — счётчики секций и
 *     состояние обмена не мигают.
 *
 * Средняя панель показывается по `route.meta.listPane`, а не по имени роута и не
 * по флагу в сторе: геометрия — функция от того, где мы находимся, и дублировать
 * её в состояние значит позволить двум правдам разойтись.
 */

const route = useRoute()

const records = useRecordsStore()
const sections = useSectionsStore()
const conflicts = useConflictsStore()
const sync = useSyncStore()
const devices = useDevicesStore()
const security = useSecurityStore()

/** Показывать ли средний столбец со списком записей. */
const showList = computed(() => route.meta.listPane === true)

onMounted(() => {
  void records.load()
  void sections.ensure()
  // Отметки в строках списка и подвал сайдбара смотрят в те же данные — `ensure`
  // второй команды не шлёт.
  void conflicts.ensure()
  void sync.ensure()
  // Счётчик в нав-строке «Устройства».
  void devices.ensure()
  // Таймауты показа секрета и очистки буфера должны действовать с первой
  // открытой карточки, а не с первого захода в настройки.
  void security.ensure()
})
</script>

<template>
  <div class="vault-shell">
    <VaultSidebar />
    <RecordList v-if="showList" />
    <RouterView />
  </div>
</template>

<style scoped>
.vault-shell {
  flex: 1;
  display: flex;
  min-width: 0;
  min-height: 0;
}

/*
 * Узкое окно: панели встают друг под друга. Прототип нарисован на 1440 и с этим
 * не сталкивается, но 238 + 376 на вьюпорте 1000px оставляют правой панели
 * меньше четырёхсот пикселей — карточке записи там уже тесно.
 */
@media (max-width: 1080px) {
  .vault-shell {
    flex-wrap: wrap;
  }
}

@media (max-width: 860px) {
  .vault-shell {
    flex-direction: column;
  }
}
</style>

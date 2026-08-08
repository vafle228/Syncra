<script setup lang="ts">
import { computed, ref } from 'vue'

import { SyButton, SyInput } from '@/components/ui'
import { pluralize } from '@/composables/plural'
import { useVaultExport } from '@/composables/useDataTransfer'
import { formatFileSize } from './importSources'

/**
 * CSV-экспорт (F12, §6.2 — «опасный вариант»).
 *
 * Формулировки предупреждений взяты из спека и макета и НЕ смягчены: это прямое
 * требование §6.2 и CLAUDE.md. Файл собирает ядро — все пароли хранилища
 * открытым текстом через границу IPC не проходят даже на секунду.
 *
 * Подтверждений три, и каждое про своё: галочка — обещание удалить файл, слово
 * ЭКСПОРТ — защита от случайного нажатия, мастер-пароль — доказательство, что
 * за клавиатурой хозяин хранилища, а не тот, кто подошёл к открытому ноутбуку.
 */

const props = defineProps<{ records: number }>()

/** Слово подтверждения из макета. Сверяется без учёта регистра и пробелов. */
const CONFIRM_WORD = 'ЭКСПОРТ'

const csv = useVaultExport('csv')
const acknowledged = ref(false)
const word = ref('')
const masterPassword = ref('')

const risks = [
  'Пароли, логины и заметки лежат в файле открытым текстом — их прочитает любая программа на компьютере.',
  'Если файл попадёт в облачную папку, копия уедет на чужой сервер и останется там в истории версий.',
  'TOTP-ключи в CSV не переносятся: их придётся подключить в новом менеджере заново.',
]

const wordMatches = computed(() => word.value.trim().toUpperCase() === CONFIRM_WORD)
const ready = computed(
  () => acknowledged.value && wordMatches.value && masterPassword.value !== '' && !csv.busy.value,
)

async function exportCsv(): Promise<void> {
  if (!ready.value) return

  const attempt = masterPassword.value
  // Пароль ушёл в ядро — здесь он больше не нужен.
  masterPassword.value = ''
  if (await csv.run(attempt)) {
    // Подтверждения одноразовые: следующий такой файл — следующее решение.
    acknowledged.value = false
    word.value = ''
  }
}

async function removeFile(): Promise<void> {
  await csv.remove()
}

const buttonLabel = computed(
  () => `Экспортировать ${pluralize(props.records, ['пароль', 'пароля', 'паролей'])} в CSV`,
)
</script>

<template>
  <section class="csv">
    <span class="csv__tag">Опасный вариант</span>
    <h3 class="csv__title">CSV без шифрования</h3>
    <p class="csv__lead">
      Нужен ровно для одного: переехать в другой менеджер, который умеет читать только таблицу. В
      файле все пароли и заметки открытым текстом.
    </p>

    <ul class="csv__risks">
      <li v-for="risk in risks" :key="risk" class="csv__risk">
        <span class="csv__risk-dot" aria-hidden="true" />
        <span>{{ risk }}</span>
      </li>
    </ul>

    <!-- Файл уже создан: карточка не закрывается, пока он лежит на диске. -->
    <div v-if="csv.file.value" class="csv__done">
      <span class="csv__done-title">Открытый файл лежит на диске</span>
      <span class="csv__done-path">
        {{ csv.file.value.path }} · {{ formatFileSize(csv.file.value.size_bytes) }}
      </span>
      <span class="csv__done-body">
        Файл не зашифрован. Удалите его сразу после использования — пока он на диске, ваши пароли
        читает любая программа на этом компьютере.
      </span>
      <div class="csv__actions">
        <SyButton variant="danger" :loading="csv.busy.value" @click="removeFile">
          Удалить файл сейчас
        </SyButton>
      </div>
    </div>

    <template v-else>
      <label class="csv__ack" :class="{ 'csv__ack--on': acknowledged }">
        <input v-model="acknowledged" type="checkbox" class="csv__ack-box" />
        <span>Я удалю файл сразу после переноса и не оставлю его в облачной папке.</span>
      </label>

      <SyInput
        v-model="word"
        label="Наберите слово ЭКСПОРТ, чтобы подтвердить"
        mono
        :disabled="!acknowledged"
        placeholder="ЭКСПОРТ"
      />

      <SyInput
        v-model="masterPassword"
        label="Мастер-пароль"
        type="password"
        autocomplete="current-password"
        :disabled="!acknowledged || !wordMatches"
        hint="Экспорт всегда требует мастер-пароль — даже если хранилище уже открыто."
        @submit="exportCsv"
      />

      <div class="csv__actions">
        <SyButton variant="danger" :disabled="!ready" :loading="csv.busy.value" @click="exportCsv">
          {{ buttonLabel }}
        </SyButton>
      </div>
    </template>

    <p v-if="csv.error.value" class="csv__error" role="alert">{{ csv.error.value }}</p>

    <p class="csv__note">
      Файл создаётся локально и никуда не отправляется. Резервной копией он не считается: для этого
      есть зашифрованный бэкап.
    </p>
  </section>
</template>

<style scoped>
.csv {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-8);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius);
  /* Штриховка из макета: опасное отличается фактурой, а не только цветом. */
  background-color: var(--sy-danger-quiet);
  background-image: repeating-linear-gradient(
    135deg,
    color-mix(in oklab, var(--sy-danger) 8%, transparent) 0 9px,
    transparent 9px 18px
  );
}

.csv__tag {
  align-self: flex-start;
  padding: 2px var(--sy-space-4);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius-pill);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--sy-danger);
}

.csv__title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.015em;
}

.csv__lead {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.csv__risks {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  margin: 0;
  padding: 0;
  list-style: none;
}

.csv__risk {
  display: flex;
  gap: var(--sy-space-5);
  align-items: flex-start;
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.csv__risk-dot {
  flex: none;
  width: 6px;
  height: 6px;
  margin-top: 6px;
  border-radius: 50%;
  background: var(--sy-danger);
}

.csv__ack {
  display: flex;
  gap: var(--sy-space-5);
  align-items: flex-start;
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-bg-0);
  font-size: 12.5px;
  line-height: 1.5;
  cursor: pointer;
}

.csv__ack--on {
  border-color: var(--sy-danger);
}

.csv__ack-box {
  flex: none;
  width: 15px;
  height: 15px;
  margin-top: 1px;
  accent-color: var(--sy-danger);
}

.csv__done {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  padding: var(--sy-space-6) var(--sy-space-7);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-bg-0);
}

.csv__done-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
  color: var(--sy-danger);
}

.csv__done-path {
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-2);
  word-break: break-all;
}

.csv__done-body {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.csv__actions {
  display: flex;
  gap: var(--sy-space-4);
}

.csv__note {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.csv__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>

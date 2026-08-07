<script setup lang="ts">
import { ref } from 'vue'

import { SyButton, SyInput } from '@/components/ui'
import { PAIRING_MANUAL_CODE_LENGTH } from '@/core/contract'

/**
 * Чтение кода со второго устройства (F8, §3.6 макета).
 *
 * Почему здесь не камера. Распознавание QR требует декодера в кадре — это
 * отдельная работа мобильной части (§9), и на десктопе камеры часто просто нет.
 * Рисовать рамку с бегающей полоской, которая ничего не читает, было бы враньём:
 * человек стоял бы перед экраном и ждал. Поэтому рамка подписана честно, а
 * рабочих пути два — набрать шесть символов или открыть файл, в который код
 * сохранило второе устройство.
 *
 * Прочитанное уходит наружу событием и здесь не сохраняется: разбирать код —
 * работа ядра, а хранить его лишнюю секунду незачем.
 */

defineProps<{ busy?: boolean }>()

const emit = defineEmits<{ submit: [payload: string] }>()

const code = ref('')
const fileError = ref<string | null>(null)
const fileInput = ref<HTMLInputElement | null>(null)

function submitCode(): void {
  const value = code.value
  if (value.trim() === '') return

  // Поле чистим сразу: код прочитан, держать его на экране не нужно.
  code.value = ''
  fileError.value = null
  emit('submit', value)
}

/** Ограничение на размер файла: код — это строка, а не архив. */
const MAX_FILE_BYTES = 8 * 1024

/**
 * Через `FileReader`, а не `file.text()`: второй появился позже и есть не во
 * всех вебвью, на которых нам предстоит запускаться.
 */
function readText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('read failed'))
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.readAsText(file)
  })
}

async function onFile(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  // Значение сбрасываем, чтобы тот же файл можно было выбрать повторно.
  input.value = ''
  if (!file) return

  fileError.value = null
  if (file.size > MAX_FILE_BYTES) {
    fileError.value = 'Слишком большой файл: код — это одна строка.'
    return
  }

  try {
    emit('submit', await readText(file))
  } catch {
    fileError.value = 'Не удалось прочитать файл.'
  }
}
</script>

<template>
  <div class="scan">
    <div class="scan__frame">
      <span class="scan__corner scan__corner--tl" aria-hidden="true" />
      <span class="scan__corner scan__corner--tr" aria-hidden="true" />
      <span class="scan__corner scan__corner--bl" aria-hidden="true" />
      <span class="scan__corner scan__corner--br" aria-hidden="true" />
      <p class="scan__frame-text">
        Камера здесь код не читает — распознавание появится в мобильной версии. Наберите шесть
        символов или откройте файл с кодом.
      </p>
    </div>

    <div class="scan__form">
      <SyInput
        v-model="code"
        label="Код со второго устройства"
        mono
        placeholder="4TQ · 9MB"
        autocomplete="off"
        :disabled="busy"
        :error="fileError"
        :hint="`${PAIRING_MANUAL_CODE_LENGTH} символов, регистр и разделители не важны`"
        @submit="submitCode"
      />

      <div class="scan__actions">
        <SyButton
          variant="primary"
          size="sm"
          :disabled="code.trim() === '' || busy"
          @click="submitCode"
        >
          Прочитать код
        </SyButton>

        <SyButton size="sm" :disabled="busy" @click="fileInput?.click()">Из файла</SyButton>
        <input
          ref="fileInput"
          class="scan__file"
          type="file"
          accept=".txt,.syncra,text/plain"
          aria-label="Файл с кодом сопряжения"
          @change="onFile"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.scan {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sy-space-7);
  width: 100%;
  max-width: 300px;
}

.scan__frame {
  position: relative;
  display: grid;
  place-items: center;
  width: 100%;
  aspect-ratio: 1;
  padding: var(--sy-space-8);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-surface);
}

.scan__frame-text {
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-3);
  text-align: center;
  text-wrap: pretty;
}

.scan__corner {
  position: absolute;
  width: 34px;
  height: 34px;
  border: 0 solid var(--sy-border-strong);
}

.scan__corner--tl {
  top: 16px;
  left: 16px;
  border-top-width: 2px;
  border-left-width: 2px;
  border-top-left-radius: var(--sy-radius-sm);
}

.scan__corner--tr {
  top: 16px;
  right: 16px;
  border-top-width: 2px;
  border-right-width: 2px;
  border-top-right-radius: var(--sy-radius-sm);
}

.scan__corner--bl {
  bottom: 16px;
  left: 16px;
  border-bottom-width: 2px;
  border-left-width: 2px;
  border-bottom-left-radius: var(--sy-radius-sm);
}

.scan__corner--br {
  bottom: 16px;
  right: 16px;
  border-bottom-width: 2px;
  border-right-width: 2px;
  border-bottom-right-radius: var(--sy-radius-sm);
}

.scan__form {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  width: 100%;
}

.scan__actions {
  display: flex;
  gap: var(--sy-space-3);
}

.scan__file {
  display: none;
}
</style>

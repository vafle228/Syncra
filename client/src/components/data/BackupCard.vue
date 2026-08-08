<script setup lang="ts">
import { ref } from 'vue'

import { SyButton, SyInput } from '@/components/ui'
import { useVaultExport } from '@/composables/useDataTransfer'
import { formatFileSize } from './importSources'

/**
 * Зашифрованный бэкап (F12, §6.2 — «спокойный вариант»).
 *
 * Файл собирает и шифрует ЯДРО тем же мастер-паролем; сюда возвращается только
 * путь и размер. Мастер-пароль уходит транзитом и в состоянии не остаётся.
 */

defineProps<{
  /** Сколько записей и секций уедет в файл — числа из хранилища. */
  records: number
  vaults: number
}>()

const backup = useVaultExport('backup')
const masterPassword = ref('')

async function save(): Promise<void> {
  const attempt = masterPassword.value
  // Пароль ушёл в ядро — здесь он больше не нужен.
  masterPassword.value = ''
  await backup.run(attempt)
}
</script>

<template>
  <section class="backup">
    <span class="backup__tag">Спокойный вариант</span>
    <h3 class="backup__title">Зашифрованный бэкап</h3>
    <p class="backup__lead">
      Хранилище целиком, закрытое тем же мастер-паролем. Без него файл — бесполезный набор байтов,
      поэтому его не страшно держать на флешке или на внешнем диске.
    </p>

    <dl class="backup__specs">
      <div class="backup__spec">
        <dt>Формат</dt>
        <dd>.syncra</dd>
      </div>
      <div class="backup__spec">
        <dt>Чем закрыт</dt>
        <dd>мастер-пароль</dd>
      </div>
      <div class="backup__spec">
        <dt>Что внутри</dt>
        <dd>{{ records }} записей · {{ vaults }} секций</dd>
      </div>
    </dl>

    <div v-if="backup.file.value" class="backup__done">
      <span class="backup__done-title">Бэкап сохранён</span>
      <span class="backup__done-path">
        {{ backup.file.value.path }} · {{ formatFileSize(backup.file.value.size_bytes) }}
      </span>
    </div>

    <SyInput
      v-model="masterPassword"
      label="Мастер-пароль"
      type="password"
      autocomplete="current-password"
      hint="Экспорт всегда требует мастер-пароль — даже если хранилище уже открыто."
      :error="backup.error.value"
      @submit="save"
    />

    <div class="backup__actions">
      <SyButton variant="primary" :loading="backup.busy.value" @click="save">
        {{ backup.file.value ? 'Сохранить ещё раз' : 'Сохранить бэкап' }}
      </SyButton>
    </div>

    <p class="backup__note">
      Если сменить мастер-пароль, старый бэкап продолжит открываться старым — держите это в голове.
    </p>
    <p class="backup__note">
      Восстанавливают из такого файла на чистом устройстве: если Syncra на нём уже есть, хранилища
      придётся не сливать, а знакомить устройства по QR.
    </p>
  </section>
</template>

<style scoped>
.backup {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-8);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
}

.backup__tag {
  align-self: flex-start;
  padding: 2px var(--sy-space-4);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-pill);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--sy-accent);
}

.backup__title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.015em;
}

.backup__lead {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.backup__specs {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  margin: 0;
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
}

.backup__spec {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-5);
  font-size: 12.5px;
}

.backup__spec dt {
  color: var(--sy-text-3);
}

.backup__spec dd {
  margin: 0;
  font-family: var(--sy-font-mono);
  font-size: 11.5px;
}

.backup__done {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-accent-quiet);
}

.backup__done-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.backup__done-path {
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-2);
  word-break: break-all;
}

.backup__actions {
  display: flex;
  gap: var(--sy-space-4);
}

.backup__note {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}
</style>

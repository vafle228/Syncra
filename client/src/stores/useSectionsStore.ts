import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import type { Vault, VaultColor, VaultId, VaultPatch } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Секции хранилища (F7, §4.2).
 *
 * В контракте они называются vaults, в интерфейсе — «секции»: слово «хранилище»
 * в продукте уже занято тем единственным зашифрованным файлом, который
 * открывается мастер-паролем. Стор назван по интерфейсному слову, чтобы его не
 * путали с `useVaultStore` — тот про замок, а не про папки.
 *
 * ЗАКОН №1: имя, цвет и флаг синхронизации — метаданные. Секретов здесь нет и
 * появиться неоткуда: `Vault` их не содержит по контракту. Но состав секций —
 * это содержимое хранилища, поэтому по событию `locked` список очищается, как
 * и список записей.
 */
export const useSectionsStore = defineStore('sections', () => {
  const vaults = ref<Vault[]>([])
  const loading = ref(false)
  /** Был ли хоть один успешный ответ ядра. */
  const loaded = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)

  /** Секция по умолчанию — туда ложится запись, созданная без выбора. */
  const defaultVault = computed(
    () => vaults.value.find((vault) => vault.is_default) ?? vaults.value[0] ?? null,
  )

  /** Есть ли хоть одна секция, которая не уезжает с устройства. */
  const hasLocal = computed(() => vaults.value.some((vault) => !vault.sync))

  function byId(vaultId: VaultId | null | undefined): Vault | null {
    if (vaultId == null) return null
    return vaults.value.find((vault) => vault.vault_id === vaultId) ?? null
  }

  let unsubscribe: Unsubscribe | null = null

  function watchCore(): void {
    if (unsubscribe) return
    unsubscribe = useCore().on('locked', () => {
      clear()
    })
  }

  function clear(): void {
    vaults.value = []
    loaded.value = false
    error.value = null
  }

  /**
   * Забрать секции у ядра. Повторные вызовы ничего не делают: экраны зовут
   * `ensure()` при монтировании, а секции меняются заметно реже, чем открываются
   * карточки.
   */
  async function ensure(): Promise<Vault[]> {
    if (loaded.value) return vaults.value
    return load()
  }

  async function load(): Promise<Vault[]> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      vaults.value = await useCore().listVaults()
      loaded.value = true
    } catch (cause) {
      vaults.value = []
      loaded.value = false
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить список секций.'
    } finally {
      loading.value = false
    }
    return vaults.value
  }

  // ---------------------------------------------------------------------------
  // Изменения
  //
  // Ошибки летят наружу исключением и НЕ пишутся в `error`: он гасит список и
  // показывает вместо него сообщение. Отказ на конкретной секции должен быть
  // виден рядом с ней, а не вместо всего экрана.
  // ---------------------------------------------------------------------------

  function replace(next: Vault): void {
    vaults.value = vaults.value.map((vault) => (vault.vault_id === next.vault_id ? next : vault))
  }

  async function create(name: string, color: VaultColor): Promise<Vault> {
    const created = await useCore().createVault(name, color)
    vaults.value = [...vaults.value, created]
    return created
  }

  /** Переименовать / перекрасить. Флаг синхронизации сюда не входит намеренно. */
  async function update(vaultId: VaultId, patch: VaultPatch): Promise<Vault> {
    const updated = await useCore().updateVault(vaultId, patch)
    replace(updated)
    return updated
  }

  /** Тумблер §4.2: уезжает секция на другие устройства или остаётся здесь. */
  async function setSync(vaultId: VaultId, sync: boolean): Promise<Vault> {
    const updated = await useCore().setVaultSync(vaultId, sync)
    replace(updated)
    return updated
  }

  async function setDefault(vaultId: VaultId): Promise<Vault[]> {
    vaults.value = await useCore().setDefaultVault(vaultId)
    return vaults.value
  }

  /**
   * Удалить секцию. Записи при этом не удаляются — ядро переносит их в секцию
   * по умолчанию, поэтому список записей после этого надо перечитать.
   */
  async function remove(vaultId: VaultId): Promise<Vault[]> {
    vaults.value = await useCore().deleteVault(vaultId)
    return vaults.value
  }

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return {
    vaults,
    loading,
    loaded,
    error,
    defaultVault,
    hasLocal,
    byId,
    ensure,
    load,
    create,
    update,
    setSync,
    setDefault,
    remove,
    clear,
    dispose,
  }
})

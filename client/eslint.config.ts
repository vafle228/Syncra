import { globalIgnores } from 'eslint/config'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import pluginVue from 'eslint-plugin-vue'
import pluginVitest from '@vitest/eslint-plugin'
import skipFormatting from '@vue/eslint-config-prettier/skip-formatting'

export default defineConfigWithVueTs(
  {
    name: 'app/files-to-lint',
    files: ['**/*.{ts,mts,tsx,vue}'],
  },

  // `design/` — выгрузка макетов из Claude Design, не наш исходник.
  globalIgnores([
    '**/dist/**',
    '**/dist-ssr/**',
    '**/coverage/**',
    '**/node_modules/**',
    'design/**',
  ]),

  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,

  {
    ...pluginVitest.configs.recommended,
    files: ['src/**/__tests__/*'],
    rules: {
      ...pluginVitest.configs.recommended.rules,
      'vitest/expect-expect': [
        'error',
        { assertFunctionNames: ['expect', 'expectCoreError', 'expectNoSecrets'] },
      ],
    },
  },

  {
    name: 'app/rules',
    rules: {
      // Закон №1: компоненты не ходят в ядро мимо типизированного IPC-клиента.
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/core',
              message: 'Tauri invoke() вызывается только из src/core/ipc.ts. Используй useCore().',
            },
          ],
        },
      ],
      // Секреты не должны попадать в консоль ни при каких обстоятельствах.
      'no-console': ['error', { allow: ['warn', 'error'] }],
    },
  },

  {
    name: 'app/core-ipc-exception',
    files: ['src/core/ipc.ts'],
    rules: { 'no-restricted-imports': 'off' },
  },

  skipFormatting,
)

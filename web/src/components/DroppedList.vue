<script setup lang="ts">
// Recently dropped by bank (docs/spec/m2-bank-data.md#rows-the-bank-stops-sending): pending rows
// the household had edited that the bank stopped sending, the latest first, each with Restore for
// 30 days after it went. A restored row opens in its detail *(agent)*.
import { ref } from 'vue'
import { ApiError } from '@/api/client'
import { useDropped, useFlagActions } from '@/api/syncFlags'
import Money from '@/components/Money.vue'
import { formatDate, useFormatting } from '@/lib/format'

const emit = defineEmits<{ restored: [id: number] }>()

const dropped = useDropped(true)
const actions = useFlagActions()
const formatting = useFormatting()
const failure = ref('')
const busy = ref<number>()

async function restore(id: number) {
  failure.value = ''
  busy.value = id
  try {
    const row = await actions.restore(id)
    emit('restored', row.id)
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  } finally {
    busy.value = undefined
  }
}
</script>

<template>
  <section aria-labelledby="dropped-title" class="flex flex-col gap-2">
    <h2 id="dropped-title" class="font-medium">Recently dropped by bank</h2>
    <p class="text-sm text-muted-foreground">
      Pending transactions you had edited that the bank stopped sending. Restore one within 30 days
      to keep it.
    </p>
    <p v-if="dropped.isError.value" role="alert" class="text-sm text-destructive">
      {{ dropped.error.value?.message }}
    </p>
    <p v-if="failure" role="alert" class="text-sm text-destructive">{{ failure }}</p>
    <p
      v-if="dropped.isSuccess.value && dropped.data.value?.length === 0"
      class="text-sm text-muted-foreground"
    >
      Nothing dropped lately.
    </p>
    <ul v-else class="divide-y rounded-xl border">
      <li
        v-for="row in dropped.data.value ?? []"
        :key="row.id"
        class="flex items-center gap-3 px-4 py-3"
      >
        <span class="flex min-w-0 flex-1 flex-col">
          <span class="truncate">{{ row.payee ?? row.raw_description }}</span>
          <span class="truncate text-xs text-muted-foreground">
            {{ formatDate(row.date, formatting) }} · {{ row.account }}
            <template v-if="row.category"> · {{ row.category }}</template>
            <template v-else-if="row.split"> · Split</template>
            <template v-if="row.memo"> · {{ row.memo }}</template>
          </span>
        </span>
        <Money :minor="row.amount_minor" signed />
        <button
          type="button"
          class="rounded-md border px-3 py-1"
          :disabled="busy === row.id"
          @click="restore(row.id)"
        >
          Restore
        </button>
      </li>
    </ul>
  </section>
</template>

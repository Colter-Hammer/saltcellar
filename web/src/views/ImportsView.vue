<script setup lang="ts">
// Settings › Imports (docs/spec/m1-ledger-import.md#settings--imports): the import batches, and
// Undo on an active one. Undo goes straight ahead unless rows were edited since; then the screen
// says so and asks, in place, before it discards those edits *(agent)*.
import { computed, ref } from 'vue'
import { ApiError } from '@/api/client'
import { useHousehold } from '@/api/household'
import { type ImportBatch, useImportBatches, useUndoImport } from '@/api/imports'
import { formatInstantDay, useFormatting } from '@/lib/format'

const batches = useImportBatches()
const household = useHousehold()
const formatting = useFormatting()
const undo = useUndoImport()

const list = computed(() => batches.data.value?.batches ?? [])

const FORMATS: Record<string, string> = { actual: 'Actual', csv: 'CSV', ofx: 'OFX' }

function day(batch: ImportBatch): string {
  const zone = household.data.value?.timezone ?? 'UTC'
  return formatInstantDay(batch.imported_at, zone, formatting.value)
}

/** The batch Undo is asking about, and what it said. */
const asking = ref<{ id: number; message: string } | null>(null)
/** Why an Undo failed. */
const failure = ref('')

async function run(batch: ImportBatch, confirm: boolean) {
  failure.value = ''
  try {
    await undo.mutateAsync({ id: batch.id, confirm })
    asking.value = null
  } catch (error) {
    if (error instanceof ApiError && error.code === 'rows_edited') {
      asking.value = { id: batch.id, message: error.message }
    } else {
      asking.value = null
      failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
    }
  }
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p v-if="batches.isError.value" role="alert" class="text-sm text-destructive">
      {{ batches.error.value?.message }}
    </p>
    <p v-if="failure" role="alert" class="text-sm text-destructive">{{ failure }}</p>

    <p v-if="batches.isSuccess.value && list.length === 0" class="text-sm text-muted-foreground">
      Nothing has been imported yet.
    </p>

    <ul v-if="list.length > 0" aria-label="Imports" class="divide-y rounded-xl border lg:max-w-3xl">
      <li v-for="batch in list" :key="batch.id" class="flex flex-col gap-2 p-3">
        <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span class="font-medium">{{ day(batch) }}</span>
          <span class="text-sm text-muted-foreground">
            {{ FORMATS[batch.format] ?? batch.format }} · {{ batch.file_name }}
          </span>
        </div>
        <p class="text-sm text-muted-foreground">
          <template v-if="batch.imported_by">By {{ batch.imported_by }} · </template>
          <span class="tabular-nums">{{ batch.added }}</span> added,
          <span class="tabular-nums">{{ batch.matched }}</span> matched
        </p>

        <p v-if="batch.state === 'undone'" class="text-sm">Undone</p>
        <div v-else class="flex flex-col items-start gap-2">
          <p class="text-sm">Active</p>
          <template v-if="asking?.id === batch.id">
            <p role="alert" class="text-sm">{{ asking.message }}</p>
            <div class="flex flex-wrap gap-2">
              <button
                type="button"
                class="rounded-md bg-destructive px-3 py-1 text-white disabled:opacity-50"
                :disabled="undo.isPending.value"
                @click="run(batch, true)"
              >
                Undo anyway
              </button>
              <button type="button" class="rounded-md border px-3 py-1" @click="asking = null">
                Cancel
              </button>
            </div>
          </template>
          <button
            v-else
            type="button"
            class="rounded-md border px-3 py-1 disabled:opacity-50"
            :disabled="undo.isPending.value"
            @click="run(batch, false)"
          >
            Undo
          </button>
        </div>
      </li>
    </ul>
  </div>
</template>

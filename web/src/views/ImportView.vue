<script setup lang="ts">
// Import from Actual (docs/spec/m1-ledger-import.md#import-from-actual): file → accounts →
// categories → done. Step 1 reads the export and says what's in it; nothing is imported until the
// last step. Steps 2 to 4 arrive in later issues; until then Next leads to a placeholder.
import { computed, ref } from 'vue'
import { type ActualUpload, uploadActualExport } from '@/api/imports'
import { useFormatting } from '@/lib/format'
import { describeUpload } from '@/lib/import'

const steps = ['File', 'Accounts', 'Categories', 'Done']
const step = ref(1)

const formatting = useFormatting()
const fileName = ref('')
const upload = ref<ActualUpload | null>(null)
const reading = ref(false)
const error = ref('')
const summary = computed(() => upload.value && describeUpload(upload.value, formatting.value))

async function choose(event: Event) {
  const input = event.target
  if (!(input instanceof HTMLInputElement)) return
  const file = input.files?.[0]
  if (file === undefined) return
  fileName.value = file.name
  upload.value = null
  error.value = ''
  reading.value = true
  try {
    upload.value = await uploadActualExport(file)
  } catch (failure) {
    error.value = failure instanceof Error ? failure.message : String(failure)
  } finally {
    reading.value = false
    // So choosing the same file again reads it again.
    input.value = ''
  }
}
</script>

<template>
  <div class="flex max-w-3xl flex-col gap-5 p-4 lg:px-6">
    <ol aria-label="Steps" class="flex gap-1 text-xs">
      <li
        v-for="(name, i) in steps"
        :key="name"
        class="flex-1"
        :aria-current="i + 1 === step ? 'step' : undefined"
      >
        <div class="h-1 rounded-full" :class="i + 1 <= step ? 'bg-primary' : 'bg-muted'" />
        <div class="mt-1" :class="i + 1 === step ? 'font-medium' : 'text-muted-foreground'">
          {{ i + 1 }}. {{ name }}
        </div>
      </li>
    </ol>

    <section v-if="step === 1" class="flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="actual-export" class="text-sm font-medium">Actual export (.zip)</label>
        <p id="actual-export-hint" class="text-sm text-muted-foreground">
          In Actual, use Settings › Export data, then choose the .zip it saves. Nothing is imported
          until the last step.
        </p>
        <input
          id="actual-export"
          type="file"
          accept=".zip,application/zip"
          aria-describedby="actual-export-hint"
          class="rounded-md border p-2 text-sm"
          :disabled="reading"
          @change="choose"
        />
      </div>

      <p v-if="reading" role="status" class="text-sm text-muted-foreground">
        Reading {{ fileName }}…
      </p>
      <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>

      <section
        v-if="upload && summary"
        aria-label="The export"
        class="flex flex-col gap-1 rounded-xl border p-4 text-sm"
      >
        <h2 class="font-medium">{{ upload.budget_name }}</h2>
        <p>{{ summary.counts }}</p>
        <p>{{ summary.dates }}</p>
        <p class="text-muted-foreground">{{ fileName }}</p>
      </section>

      <div class="flex justify-end">
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-2 text-primary-foreground disabled:opacity-50"
          :disabled="upload === null"
          @click="step = 2"
        >
          Next
        </button>
      </div>
    </section>

    <section v-else class="flex flex-col gap-4">
      <p class="rounded-xl border border-dashed p-4 text-sm text-muted-foreground">
        Choosing what happens to each Actual account comes next, in a later version.
      </p>
      <div class="flex justify-start">
        <button type="button" class="rounded-md border px-3 py-2" @click="step = 1">Back</button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
// Fixing a split whose amount the bank changed (docs/spec/m2-bank-data.md#who-owns-which-field):
// each line's category, amount and memo, lines added or removed, with what's left to split
// shown, until the lines add up *(agent)*. Review takes this over in milestone 3.
import { computed, ref, watch } from 'vue'
import { ApiError } from '@/api/client'
import { useFlagActions } from '@/api/syncFlags'
import type { Transaction } from '@/api/transactions'
import CategoryPicker from '@/components/CategoryPicker.vue'
import Money from '@/components/Money.vue'
import { useFormatting } from '@/lib/format'
import { readSplit, type SplitLineFields, splitFieldsOf, splitRemaining } from '@/lib/syncFlags'

const props = defineProps<{ transaction: Transaction }>()
const emit = defineEmits<{ done: []; cancel: [] }>()

const formatting = useFormatting()
const actions = useFlagActions()
const lines = ref<SplitLineFields[]>(splitFieldsOf(props.transaction, formatting.value))
const left = computed(() =>
  splitRemaining(lines.value, props.transaction.amount_minor, formatting.value),
)
const problem = ref('')
const saving = ref(false)
// A problem goes once the lines change.
watch(lines, () => (problem.value = ''), { deep: true })

function add() {
  lines.value.push({ category: '', amount: '', memo: '' })
}

async function save() {
  const read = readSplit(lines.value, props.transaction.amount_minor, formatting.value)
  if (typeof read === 'string') {
    problem.value = read
    return
  }
  problem.value = ''
  saving.value = true
  try {
    await actions.fixSplit(props.transaction.id, read)
    emit('done')
  } catch (error) {
    problem.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <form aria-label="Fix the split" class="flex min-w-0 flex-col gap-3" @submit.prevent="save">
    <fieldset
      v-for="(line, index) in lines"
      :key="index"
      class="flex min-w-0 flex-col gap-1 rounded-md border p-2"
    >
      <legend class="px-1 text-xs text-muted-foreground">Line {{ index + 1 }}</legend>
      <CategoryPicker
        v-model="line.category"
        :label="`Line ${String(index + 1)} category`"
        placeholder="Choose a category…"
      />
      <div class="flex items-center gap-2">
        <input
          v-model="line.amount"
          inputmode="decimal"
          :aria-label="`Line ${String(index + 1)} amount`"
          class="w-28 rounded-md border px-2 py-1"
        />
        <input
          v-model="line.memo"
          :aria-label="`Line ${String(index + 1)} memo`"
          placeholder="Memo"
          class="min-w-0 flex-1 rounded-md border px-2 py-1"
        />
        <button
          v-if="lines.length > 1"
          type="button"
          class="px-1 text-sm text-muted-foreground hover:underline"
          @click="lines.splice(index, 1)"
        >
          Remove
        </button>
      </div>
    </fieldset>
    <p class="flex items-center gap-2 text-sm" data-testid="split-left">
      Left to split: <Money :minor="left" signed />
    </p>
    <div class="flex flex-wrap gap-2">
      <button type="button" class="rounded-md border px-3 py-1" @click="add">Add a line</button>
      <button
        type="submit"
        class="rounded-md bg-primary px-3 py-1 text-primary-foreground"
        :disabled="saving"
      >
        Save split
      </button>
      <button type="button" class="rounded-md border px-3 py-1" @click="emit('cancel')">
        Cancel
      </button>
    </div>
    <p v-if="problem" role="alert" class="text-destructive">{{ problem }}</p>
  </form>
</template>

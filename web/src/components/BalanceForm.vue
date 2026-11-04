<script setup lang="ts">
// Update balance on an account with no connection (docs/spec/m2-bank-data.md#manual-entry): what's
// in it today, recorded as a manual balance snapshot. It starts empty so a stray Save can't
// re-record the old balance *(agent)*.
import { ref } from 'vue'
import FormField from '@/components/FormField.vue'
import { type Formatting, formatMoney, parseMoney } from '@/lib/format'

const props = defineProps<{ formatting: Formatting }>()
const emit = defineEmits<{ save: [balanceMinor: number]; cancel: [] }>()

const balance = ref('')
const error = ref<string>()

function submit() {
  const minor = parseMoney(balance.value, props.formatting)
  if (minor === null) {
    error.value = `Enter an amount, like ${formatMoney(123456, props.formatting)}.`
    return
  }
  error.value = undefined
  emit('save', minor)
}
</script>

<template>
  <form
    aria-label="Update balance"
    class="flex flex-col gap-3 rounded-xl border p-4 lg:max-w-md"
    @submit.prevent="submit"
  >
    <FormField
      v-model="balance"
      name="balance"
      label="Balance today"
      :error="error"
      inputmode="decimal"
      autocomplete="off"
      required
    />
    <div class="flex gap-2">
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Save balance
      </button>
      <button type="button" class="rounded-md border px-3 py-2" @click="emit('cancel')">
        Cancel
      </button>
    </div>
  </form>
</template>

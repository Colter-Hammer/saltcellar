<script setup lang="ts">
// A one-line form for a name: typed in place of what it names. Enter saves, Escape cancels.
import { onMounted, ref, useTemplateRef } from 'vue'

const props = defineProps<{
  /** What the input is called for screen readers, e.g. "New name for Groceries". */
  label: string
  initial?: string
  /** The save button's text. */
  action: string
  error?: string | undefined
}>()
const emit = defineEmits<{ save: [name: string]; cancel: [] }>()

const name = ref(props.initial ?? '')
const input = useTemplateRef<HTMLInputElement>('input')
onMounted(() => {
  input.value?.select()
})
</script>

<template>
  <form class="flex flex-1 flex-col gap-1" @submit.prevent="emit('save', name)">
    <div class="flex flex-wrap items-center gap-2">
      <input
        ref="input"
        v-model="name"
        :aria-label="label"
        :aria-invalid="error ? true : undefined"
        class="min-w-0 flex-1 rounded-md border px-2 py-1"
        required
        @keydown.escape="emit('cancel')"
      />
      <button type="submit" class="rounded-md bg-primary px-2 py-1 text-sm text-primary-foreground">
        {{ action }}
      </button>
      <button type="button" class="rounded-md border px-2 py-1 text-sm" @click="emit('cancel')">
        Cancel
      </button>
    </div>
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
  </form>
</template>

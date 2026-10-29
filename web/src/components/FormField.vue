<script setup lang="ts">
// A labelled text input with an optional hint and error, wired up for screen readers. Any other
// attribute (type, autocomplete, list, …) goes to the input.
import { computed } from 'vue'

defineOptions({ inheritAttrs: false })

const props = defineProps<{
  name: string
  label: string
  hint?: string | undefined
  error?: string | undefined
}>()
const value = defineModel<string>({ required: true })

const describedBy = computed(
  () =>
    [props.hint && `${props.name}-hint`, props.error && `${props.name}-error`]
      .filter(Boolean)
      .join(' ') || undefined,
)
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <label :for="name" class="text-sm font-medium">{{ label }}</label>
    <p v-if="hint" :id="`${name}-hint`" class="text-sm text-muted-foreground">{{ hint }}</p>
    <input
      :id="name"
      v-model="value"
      :name="name"
      :aria-invalid="error ? true : undefined"
      :aria-describedby="describedBy"
      class="rounded-md border px-3 py-2"
      v-bind="$attrs"
    />
    <p v-if="error" :id="`${name}-error`" class="text-sm text-destructive">{{ error }}</p>
  </div>
</template>

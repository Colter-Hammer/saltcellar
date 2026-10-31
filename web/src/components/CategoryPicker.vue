<script setup lang="ts">
// Pick one of the app's categories, grouped as Settings › Categories has them; hidden ones aren't
// offered. Shared: From Actual, undecided uses it now, Review in milestone 3 will too *(agent)*.
// The value is the category's id as text, or empty while none is picked.
import { computed } from 'vue'
import { useCategories } from '@/api/categories'
import { offeredCategories } from '@/lib/import'

defineProps<{
  /** The picker's accessible name. */
  label: string
  /** The empty first option's words, like "Move all to…". */
  placeholder: string
}>()
const value = defineModel<string>({ required: true })

const categories = useCategories()
const groups = computed(() => offeredCategories(categories.data.value?.groups ?? []))
</script>

<template>
  <select
    v-model="value"
    :aria-label="label"
    class="w-full max-w-64 rounded-md border bg-background px-2 py-1"
  >
    <option value="">{{ placeholder }}</option>
    <optgroup v-for="group in groups" :key="group.id" :label="group.name">
      <option v-for="category in group.categories" :key="category.id" :value="String(category.id)">
        {{ category.name }}
      </option>
    </optgroup>
  </select>
</template>

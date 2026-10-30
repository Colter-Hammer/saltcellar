<script setup lang="ts">
// One Actual category in the importer's Categories step: its group, the imported rows using it,
// and where they go. A row of the step's table on desktop; stacked on a phone.
import { computed, ref } from 'vue'
import type { CategoryGroup } from '@/api/categories'
import type { ActualCategory } from '@/api/imports'
import NewCategoryForm from '@/components/NewCategoryForm.vue'
import { goesToCategory, LATER, NEW_CATEGORY, offeredCategories, TRANSFER } from '@/lib/import'

const props = defineProps<{
  category: ActualCategory
  rows: number
  groups: readonly CategoryGroup[]
  /** Makes the row's ids unique on the page. */
  index: number
  locale: string
}>()
/** Decide later, Transfer, or `category:<id>`. */
const goesTo = defineModel<string>({ required: true })

const id = computed(() => `category-${String(props.index)}`)
const offered = computed(() => offeredCategories(props.groups))
const creating = ref(false)

function choose(event: Event) {
  const select = event.target
  if (!(select instanceof HTMLSelectElement)) return
  if (select.value === NEW_CATEGORY) {
    // + New category… opens the form; the choice stays as it was until one is created.
    select.value = goesTo.value
    creating.value = true
    return
  }
  goesTo.value = select.value
}

function created(category: number) {
  goesTo.value = goesToCategory(category)
  creating.value = false
}
</script>

<template>
  <tr role="row" class="align-top max-lg:block max-lg:border-b max-lg:py-3">
    <th
      scope="row"
      role="rowheader"
      class="py-2 pr-3 text-left font-medium max-lg:block max-lg:p-0"
    >
      {{ category.name }}
    </th>
    <td
      role="cell"
      class="py-2 pr-3 max-lg:inline max-lg:p-0 max-lg:text-sm max-lg:text-muted-foreground"
    >
      {{ category.group }}<span class="lg:hidden"> · </span>
    </td>
    <td
      role="cell"
      class="py-2 pr-3 tabular-nums max-lg:inline max-lg:p-0 max-lg:text-sm max-lg:text-muted-foreground lg:text-right"
    >
      {{ new Intl.NumberFormat(locale).format(rows) }}
      <span class="lg:hidden">{{ rows === 1 ? 'row' : 'rows' }}</span>
    </td>
    <td role="cell" class="py-2 max-lg:mt-2 max-lg:block max-lg:p-0">
      <label :for="`${id}-goes-to`" class="sr-only">{{ category.name }} goes to</label>
      <select
        :id="`${id}-goes-to`"
        :value="goesTo"
        class="w-full rounded-md border bg-background px-2 py-1 lg:w-64"
        @change="choose"
      >
        <optgroup v-for="group in offered" :key="group.id" :label="group.name">
          <option
            v-for="option in group.categories"
            :key="option.id"
            :value="goesToCategory(option.id)"
          >
            {{ option.name }}
          </option>
        </optgroup>
        <option :value="TRANSFER">Transfer (not a category)</option>
        <option :value="NEW_CATEGORY">+ New category…</option>
        <option :value="LATER">Decide later</option>
      </select>
      <NewCategoryForm
        v-if="creating"
        :id="`${id}-new`"
        :category="category"
        :groups="groups"
        @created="created"
        @cancel="creating = false"
      />
    </td>
  </tr>
</template>

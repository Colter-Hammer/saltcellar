<script setup lang="ts">
// The importer's + New category…: a name and a group, created at once
// (docs/spec/m1-ledger-import.md#step-3-categories). The group can be a new one too, named after
// the Actual group *(agent)*.
import { computed, ref } from 'vue'
import { ApiError } from '@/api/client'
import { type CategoryGroup, type CategoryGroupKind, useCategoryEdits } from '@/api/categories'
import type { ActualCategory } from '@/api/imports'
import { startingGroup } from '@/lib/import'

const props = defineProps<{
  category: ActualCategory
  groups: readonly CategoryGroup[]
  /** Makes the form's ids unique on the page. */
  id: string
}>()
const emit = defineEmits<{ created: [id: number]; cancel: [] }>()

const edits = useCategoryEdits()

const NEW_GROUP = 'new'
const shownGroups = computed(() => props.groups.filter((group) => !group.hidden))

const name = ref(props.category.name)
const start = startingGroup(props.category, props.groups)
const group = ref(start === null ? NEW_GROUP : String(start))
const groupName = ref(props.category.group)
const groupKind = ref<CategoryGroupKind>(props.category.income ? 'income' : 'expense')

const nameError = ref<string>()
const groupError = ref<string>()
const problem = ref('')
const saving = ref(false)

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

async function create() {
  nameError.value = undefined
  groupError.value = undefined
  problem.value = ''
  saving.value = true
  try {
    if (group.value === NEW_GROUP) {
      try {
        // Once made, the group is chosen, so trying again doesn't make it twice.
        group.value = String(await edits.addGroup(groupName.value, groupKind.value))
      } catch (error) {
        if (error instanceof ApiError && error.fields.name !== undefined) {
          groupError.value = error.fields.name
        } else problem.value = message(error)
        return
      }
    }
    try {
      emit('created', await edits.addCategory(Number(group.value), name.value))
    } catch (error) {
      if (error instanceof ApiError && error.fields.name !== undefined) {
        nameError.value = error.fields.name
      } else problem.value = message(error)
    }
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <form
    class="mt-2 flex flex-col gap-2 rounded-md border p-3"
    :aria-label="`New category for ${category.name}`"
    @submit.prevent="create"
  >
    <div class="flex flex-col gap-1">
      <label :for="`${id}-name`" class="text-sm font-medium">New category name</label>
      <input
        :id="`${id}-name`"
        v-model="name"
        :aria-invalid="nameError ? true : undefined"
        :aria-describedby="nameError ? `${id}-name-error` : undefined"
        class="rounded-md border px-2 py-1"
        required
        @keydown.escape="emit('cancel')"
      />
      <p v-if="nameError" :id="`${id}-name-error`" class="text-sm text-destructive">
        {{ nameError }}
      </p>
    </div>
    <div class="flex flex-col gap-1">
      <label :for="`${id}-group`" class="text-sm font-medium">In group</label>
      <select :id="`${id}-group`" v-model="group" class="rounded-md border bg-background px-2 py-1">
        <option v-for="shown in shownGroups" :key="shown.id" :value="String(shown.id)">
          {{ shown.name }}
        </option>
        <option :value="NEW_GROUP">New group…</option>
      </select>
    </div>
    <div v-if="group === NEW_GROUP" class="flex flex-wrap items-end gap-2">
      <div class="flex min-w-0 flex-1 flex-col gap-1">
        <label :for="`${id}-group-name`" class="text-sm font-medium">New group name</label>
        <input
          :id="`${id}-group-name`"
          v-model="groupName"
          :aria-invalid="groupError ? true : undefined"
          :aria-describedby="groupError ? `${id}-group-error` : undefined"
          class="rounded-md border px-2 py-1"
          required
        />
      </div>
      <select
        v-model="groupKind"
        aria-label="New group kind"
        class="rounded-md border bg-background px-2 py-1"
      >
        <option value="expense">Spending</option>
        <option value="income">Income</option>
      </select>
      <p v-if="groupError" :id="`${id}-group-error`" class="w-full text-sm text-destructive">
        {{ groupError }}
      </p>
    </div>
    <p v-if="problem" role="alert" class="text-sm text-destructive">{{ problem }}</p>
    <div class="flex gap-2">
      <button
        type="submit"
        class="rounded-md bg-primary px-2 py-1 text-sm text-primary-foreground disabled:opacity-50"
        :disabled="saving"
      >
        Create
      </button>
      <button type="button" class="rounded-md border px-2 py-1 text-sm" @click="emit('cancel')">
        Cancel
      </button>
    </div>
  </form>
</template>

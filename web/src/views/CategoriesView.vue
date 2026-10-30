<script setup lang="ts">
// Settings › Categories (docs/spec/m1-ledger-import.md#categories): groups holding categories, to
// add, rename, reorder, hide and delete. Desktop drags by the grip; every width also has Move up /
// Move down in each row's menu *(agent)*. Hidden ones wait in a Hidden section at the foot.
import { GripVertical } from '@lucide/vue'
import { computed, ref } from 'vue'
import { ApiError } from '@/api/client'
import {
  type CategoryGroup,
  type CategoryGroupKind,
  useCategories,
  useCategoryEdits,
} from '@/api/categories'
import ActionMenu, { type Action } from '@/components/ActionMenu.vue'
import NameForm from '@/components/NameForm.vue'
import {
  canMoveCategory,
  canMoveGroup,
  dropCategory,
  dropGroup,
  hiddenOnes,
  moveCategory,
  moveGroup,
  moveToGroup,
} from '@/lib/categories'

const categories = useCategories()
const edits = useCategoryEdits()

const groups = computed(() => categories.data.value?.groups ?? [])
const shown = computed(() => groups.value.filter((group) => !group.hidden))
const hidden = computed(() => hiddenOnes(groups.value))
/** Start empty only lasts until a group exists: with none, a reload offers the choice again. */
const startedEmpty = ref(false)
const empty = computed(() => categories.isSuccess.value && groups.value.length === 0)

/** The one form open: `rename-group:1`, `rename-category:11`, `add-category:1` or `add-group`. */
const editing = ref<string | null>(null)
/** The open form's name error. */
const nameError = ref<string>()
/** What went wrong outside a form; `hide` offers hiding a category that can't be deleted. */
const problem = ref<{ message: string; hide?: number } | null>(null)

function open(form: string) {
  editing.value = form
  nameError.value = undefined
  problem.value = null
}

/** Runs `work`; true if it worked. A name error goes to the open form, anything else above the list. */
async function attempt(work: () => Promise<unknown>): Promise<boolean> {
  nameError.value = undefined
  problem.value = null
  try {
    await work()
    return true
  } catch (error) {
    const name = error instanceof ApiError ? error.fields.name : undefined
    if (name !== undefined && editing.value !== null) nameError.value = name
    else problem.value = { message: error instanceof Error ? error.message : String(error) }
    return false
  }
}

async function save(work: () => Promise<unknown>) {
  if (await attempt(work)) editing.value = null
}

async function applyStarter() {
  await attempt(edits.applyStarter)
}

// ── Adding a group ──

const newGroupName = ref('')
const newGroupKind = ref<CategoryGroupKind>('expense')

function openAddGroup() {
  newGroupName.value = ''
  newGroupKind.value = 'expense'
  open('add-group')
}

// ── Each row's actions ──

function groupActions(group: CategoryGroup): Action[] {
  const empty = group.categories.length === 0
  return [
    {
      label: 'Rename',
      run: () => {
        open(`rename-group:${String(group.id)}`)
      },
    },
    {
      label: 'Move up',
      disabled: !canMoveGroup(groups.value, group.id, -1),
      run: () => void attempt(() => edits.order(moveGroup(groups.value, group.id, -1))),
    },
    {
      label: 'Move down',
      disabled: !canMoveGroup(groups.value, group.id, 1),
      run: () => void attempt(() => edits.order(moveGroup(groups.value, group.id, 1))),
    },
    { label: 'Hide', run: () => void attempt(() => edits.changeGroup(group.id, { hidden: true })) },
    {
      label: empty ? 'Delete' : 'Delete (empty groups only)',
      disabled: !empty,
      run: () => void attempt(() => edits.deleteGroup(group.id)),
    },
  ]
}

function categoryActions(group: CategoryGroup, id: number): Action[] {
  const others = shown.value.filter((other) => other.id !== group.id)
  return [
    {
      label: 'Rename',
      run: () => {
        open(`rename-category:${String(id)}`)
      },
    },
    {
      label: 'Move up',
      disabled: !canMoveCategory(groups.value, id, -1),
      run: () => void attempt(() => edits.order(moveCategory(groups.value, id, -1))),
    },
    {
      label: 'Move down',
      disabled: !canMoveCategory(groups.value, id, 1),
      run: () => void attempt(() => edits.order(moveCategory(groups.value, id, 1))),
    },
    {
      label: 'Move to group',
      disabled: others.length === 0,
      choices: others.map((other) => ({
        label: other.name,
        run: () => void attempt(() => edits.order(moveToGroup(groups.value, id, other.id))),
      })),
    },
    { label: 'Hide', run: () => void attempt(() => edits.changeCategory(id, { hidden: true })) },
    { label: 'Delete', run: () => void deleteCategory(id) },
  ]
}

/** Deletes the category; one in use can't be, and is offered hiding instead. */
async function deleteCategory(id: number) {
  problem.value = null
  try {
    await edits.deleteCategory(id)
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    const inUse = error instanceof ApiError && error.code === 'category_in_use'
    problem.value = inUse ? { message, hide: id } : { message }
  }
}

async function hideInstead(id: number) {
  await attempt(() => edits.changeCategory(id, { hidden: true }))
}

// ── Dragging, on desktop ──

let dragging: { kind: 'group' | 'category'; id: number } | null = null
/** Where the drop put things: saved once the drag is over, so the list doesn't move under it. */
let dropped: CategoryGroup[] | null = null

function startDrag(event: DragEvent, kind: 'group' | 'category', id: number) {
  dragging = { kind, id }
  dropped = null
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', `${kind}:${String(id)}`)
  }
}

/** A group dropped on a group lands in its place; a category dropped on a group goes to its end. */
function dropOnGroup(groupId: number) {
  const dragged = dragging
  if (dragged === null) return
  dropped =
    dragged.kind === 'group'
      ? dropGroup(groups.value, dragged.id, groupId)
      : moveToGroup(groups.value, dragged.id, groupId)
}

/** A category dropped on a category takes its place; a group dropped there lands on its group. */
function dropOnCategory(groupId: number, categoryId: number) {
  const dragged = dragging
  if (dragged === null) return
  dropped =
    dragged.kind === 'group'
      ? dropGroup(groups.value, dragged.id, groupId)
      : dropCategory(groups.value, dragged.id, categoryId)
}

function endDrag() {
  const order = dropped
  dragging = null
  dropped = null
  if (order !== null) void attempt(() => edits.order(order))
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:max-w-2xl lg:px-6">
    <p v-if="categories.isError.value" role="alert" class="text-sm text-destructive">
      {{ categories.error.value?.message }}
    </p>
    <div
      v-if="problem"
      role="alert"
      class="flex flex-wrap items-center gap-2 rounded-md border border-destructive p-3 text-sm"
    >
      <span class="text-destructive">{{ problem.message }}</span>
      <button
        v-if="problem.hide !== undefined"
        type="button"
        class="rounded-md border px-2 py-1"
        @click="hideInstead(problem.hide)"
      >
        Hide it
      </button>
    </div>

    <section v-if="empty && !startedEmpty" class="flex flex-col gap-3">
      <p class="text-sm text-muted-foreground">
        No categories yet. Start from a set of common ones you can rename, hide or delete, or from
        nothing.
      </p>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-2 text-primary-foreground"
          @click="applyStarter"
        >
          Use starter categories
        </button>
        <button type="button" class="rounded-md border px-3 py-2" @click="startedEmpty = true">
          Start empty
        </button>
      </div>
    </section>

    <template v-else-if="categories.isSuccess.value">
      <p class="text-sm text-muted-foreground">
        Every category sits in one group. Groups only hold categories; transactions never go in a
        group.
      </p>

      <section
        v-for="group in shown"
        :key="group.id"
        :aria-labelledby="`group-${group.id}`"
        class="rounded-xl border"
        :data-testid="`group-${group.name}`"
      >
        <div
          class="flex items-center gap-2 border-b bg-muted/50 p-2"
          @dragover.prevent
          @drop.prevent="dropOnGroup(group.id)"
        >
          <span
            draggable="true"
            :aria-label="`Drag ${group.name}`"
            class="hidden cursor-grab text-muted-foreground lg:block"
            @dragstart="startDrag($event, 'group', group.id)"
            @dragend="endDrag"
          >
            <GripVertical class="size-4" aria-hidden="true" />
          </span>
          <NameForm
            v-if="editing === `rename-group:${group.id}`"
            :label="`New name for ${group.name}`"
            :initial="group.name"
            action="Save"
            :error="nameError"
            @save="(name) => save(() => edits.changeGroup(group.id, { name }))"
            @cancel="editing = null"
          />
          <template v-else>
            <h2 :id="`group-${group.id}`" class="flex-1 font-semibold">{{ group.name }}</h2>
            <span
              v-if="group.kind === 'income'"
              class="rounded border px-1.5 text-xs text-muted-foreground"
            >
              Income
            </span>
            <ActionMenu :label="`${group.name} actions`" :actions="groupActions(group)" />
          </template>
        </div>
        <ul class="divide-y">
          <template v-for="category in group.categories" :key="category.id">
            <li
              v-if="!category.hidden"
              class="flex items-center gap-2 p-2"
              @dragover.prevent
              @drop.prevent.stop="dropOnCategory(group.id, category.id)"
            >
              <span
                draggable="true"
                :aria-label="`Drag ${category.name}`"
                class="hidden cursor-grab text-muted-foreground lg:block"
                @dragstart.stop="startDrag($event, 'category', category.id)"
                @dragend="endDrag"
              >
                <GripVertical class="size-4" aria-hidden="true" />
              </span>
              <NameForm
                v-if="editing === `rename-category:${category.id}`"
                :label="`New name for ${category.name}`"
                :initial="category.name"
                action="Save"
                :error="nameError"
                @save="(name) => save(() => edits.changeCategory(category.id, { name }))"
                @cancel="editing = null"
              />
              <template v-else>
                <span class="flex-1">{{ category.name }}</span>
                <ActionMenu
                  :label="`${category.name} actions`"
                  :actions="categoryActions(group, category.id)"
                />
              </template>
            </li>
          </template>
          <li class="p-2">
            <NameForm
              v-if="editing === `add-category:${group.id}`"
              :label="`New category in ${group.name}`"
              action="Add"
              :error="nameError"
              @save="(name) => save(() => edits.addCategory(group.id, name))"
              @cancel="editing = null"
            />
            <button
              v-else
              type="button"
              class="text-sm text-muted-foreground underline"
              :aria-label="`Add a category to ${group.name}`"
              @click="open(`add-category:${group.id}`)"
            >
              + Category
            </button>
          </li>
        </ul>
      </section>

      <form
        v-if="editing === 'add-group'"
        class="flex flex-col gap-2 rounded-xl border p-3"
        @submit.prevent="save(() => edits.addGroup(newGroupName, newGroupKind))"
      >
        <label for="new-group-name" class="text-sm font-medium">New group</label>
        <div class="flex flex-wrap items-center gap-2">
          <input
            id="new-group-name"
            v-model="newGroupName"
            :aria-invalid="nameError ? true : undefined"
            class="min-w-0 flex-1 rounded-md border px-2 py-1"
            required
            @keydown.escape="editing = null"
          />
          <select
            v-model="newGroupKind"
            aria-label="Kind"
            class="rounded-md border bg-background px-2 py-1"
          >
            <option value="expense">Spending</option>
            <option value="income">Income</option>
          </select>
          <button
            type="submit"
            class="rounded-md bg-primary px-2 py-1 text-sm text-primary-foreground"
          >
            Add
          </button>
          <button type="button" class="rounded-md border px-2 py-1 text-sm" @click="editing = null">
            Cancel
          </button>
        </div>
        <p v-if="nameError" role="alert" class="text-sm text-destructive">{{ nameError }}</p>
      </form>
      <button
        v-else
        type="button"
        class="self-start rounded-md border px-3 py-2"
        @click="openAddGroup"
      >
        + Group
      </button>

      <section
        v-if="hidden.groups.length > 0 || hidden.categories.length > 0"
        aria-labelledby="hidden-heading"
        class="flex flex-col gap-2"
      >
        <h2 id="hidden-heading" class="font-semibold">Hidden</h2>
        <ul class="divide-y rounded-xl border">
          <li
            v-for="group in hidden.groups"
            :key="`g${group.id}`"
            class="flex items-center gap-2 p-2"
          >
            <span class="flex-1">
              {{ group.name }}
              <span class="text-sm text-muted-foreground">
                · group, {{ group.categories.length }}
                {{ group.categories.length === 1 ? 'category' : 'categories' }}
              </span>
            </span>
            <button
              type="button"
              class="rounded-md border px-2 py-1 text-sm"
              :aria-label="`Unhide ${group.name}`"
              @click="attempt(() => edits.changeGroup(group.id, { hidden: false }))"
            >
              Unhide
            </button>
          </li>
          <li
            v-for="{ category, group } in hidden.categories"
            :key="`c${category.id}`"
            class="flex items-center gap-2 p-2"
          >
            <span class="flex-1">
              {{ category.name }}
              <span class="text-sm text-muted-foreground">· in {{ group.name }}</span>
            </span>
            <button
              type="button"
              class="rounded-md border px-2 py-1 text-sm"
              :aria-label="`Unhide ${category.name}`"
              @click="attempt(() => edits.changeCategory(category.id, { hidden: false }))"
            >
              Unhide
            </button>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>

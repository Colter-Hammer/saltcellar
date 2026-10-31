<script setup lang="ts">
// One Actual category in From Actual, undecided: its count and total, Move all to… a category,
// and a list of its rows that opens on request. A row opens the transaction detail.
import { computed, ref } from 'vue'
import { ApiError } from '@/api/client'
import { type UndecidedGroup, useDecide, useUndecidedRows } from '@/api/undecided'
import CategoryPicker from '@/components/CategoryPicker.vue'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'
import { payeeLabel } from '@/lib/transactions'

const props = defineProps<{ group: UndecidedGroup; selected: number | null }>()

const formatting = useFormatting()
const decide = useDecide()

// Move all to… takes a category and then the button, so a stray tap can't move a group *(agent)*.
const target = ref('')
const failure = ref('')
const name = computed(() =>
  props.group.actual_category === '' ? '(no name)' : props.group.actual_category,
)
const rowWord = computed(() => (props.group.rows === 1 ? 'row' : 'rows'))

async function moveAll() {
  failure.value = ''
  try {
    await decide.group.mutateAsync({
      actualCategory: props.group.actual_category,
      categoryId: Number(target.value),
    })
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  }
}

const open = ref(false)
const pages = useUndecidedRows(
  () => props.group.actual_category,
  () => open.value,
)
const rows = computed(() => pages.data.value?.pages.flatMap((page) => page.transactions) ?? [])
</script>

<template>
  <section :aria-label="name" class="flex flex-col gap-3 rounded-xl border p-4">
    <div class="flex flex-wrap items-baseline gap-x-3">
      <h2 class="font-medium">{{ name }}</h2>
      <p class="text-sm text-muted-foreground">
        {{ group.rows }} {{ rowWord }} ·
        <span class="tabular-nums">{{
          formatMoney(group.total_minor, formatting, { signed: true })
        }}</span>
      </p>
    </div>

    <div class="flex flex-wrap items-center gap-2">
      <CategoryPicker
        v-model="target"
        :label="`Move all of ${name} to`"
        placeholder="Move all to…"
      />
      <button
        type="button"
        class="rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-50"
        :disabled="target === '' || decide.group.isPending.value"
        @click="moveAll"
      >
        Move all
      </button>
    </div>
    <p v-if="failure" role="alert" class="text-sm text-destructive">{{ failure }}</p>

    <button
      type="button"
      class="self-start text-sm underline"
      :aria-expanded="open"
      @click="open = !open"
    >
      {{ open ? 'Hide rows' : 'Show rows' }}
    </button>
    <template v-if="open">
      <p v-if="pages.isError.value" role="alert" class="text-sm text-destructive">
        {{ pages.error.value?.message }}
      </p>
      <ul :aria-label="`${name} rows`" class="divide-y rounded-lg border">
        <li v-for="row in rows" :key="row.id">
          <RouterLink
            :to="`/undecided/${String(row.id)}`"
            class="flex items-center gap-3 px-3 py-2"
            :class="row.id === selected && 'bg-muted'"
            :aria-current="row.id === selected ? 'true' : undefined"
          >
            <span class="flex min-w-0 flex-1 flex-col">
              <span class="truncate">{{ payeeLabel(row) }}</span>
              <span class="text-xs text-muted-foreground">
                {{ formatDate(row.date, formatting) }} · {{ row.account }}
              </span>
            </span>
            <span class="shrink-0 tabular-nums">
              {{ formatMoney(row.amount_minor, formatting, { signed: true }) }}
            </span>
          </RouterLink>
        </li>
      </ul>
      <button
        v-if="pages.hasNextPage.value"
        type="button"
        class="self-start rounded-md border px-3 py-1 text-sm"
        :disabled="pages.isFetchingNextPage.value"
        @click="pages.fetchNextPage()"
      >
        Load more
      </button>
    </template>
  </section>
</template>

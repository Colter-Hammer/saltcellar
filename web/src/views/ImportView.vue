<script setup lang="ts">
// Import from Actual (docs/spec/m1-ledger-import.md#import-from-actual): file → accounts →
// categories → done. Step 1 reads the export and says what's in it; steps 2 and 3 choose what
// happens to each Actual account and category. The choices live here, in the browser, until the
// import commits *(agent)*, so Back keeps them; nothing is imported until the last step, which
// arrives in a later issue: until then Next from step 3 leads to a placeholder.
import { computed, ref } from 'vue'
import { useCategories } from '@/api/categories'
import {
  type ActualImport,
  type ActualUpload,
  getActualImport,
  uploadActualExport,
} from '@/api/imports'
import ImportAccountRow from '@/components/ImportAccountRow.vue'
import ImportCategoryRow from '@/components/ImportCategoryRow.vue'
import { useFormatting } from '@/lib/format'
import {
  type AccountChoice,
  accountProblems,
  describeUpload,
  LATER,
  startAccountChoices,
  startCategoryChoices,
  usedCategories,
} from '@/lib/import'

const steps = ['File', 'Accounts', 'Categories', 'Done']
const step = ref(1)

const formatting = useFormatting()
const categories = useCategories()
const groups = computed(() => categories.data.value?.groups ?? [])

// ── Step 1: File ──

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

// ── Steps 2 and 3: the choices ──

const plan = ref<ActualImport | null>(null)
const accountChoices = ref<Record<string, AccountChoice>>({})
/** Each Actual category's Goes to, by its id: Decide later, Transfer, or `category:<id>`. */
const categoryChoices = ref<Record<string, string>>({})
const loading = ref(false)

const problems = computed(() =>
  plan.value ? accountProblems(plan.value, accountChoices.value) : {},
)
/** Each account with its choice, in the plan's order. */
const accountRows = computed(() =>
  (plan.value?.accounts ?? []).flatMap((account) => {
    const choice = accountChoices.value[account.id]
    return choice === undefined ? [] : [{ account, choice }]
  }),
)
/** Each category the imported rows use, with its Goes to. */
const categoryRows = computed(() =>
  (plan.value ? usedCategories(plan.value, accountChoices.value) : []).map((used) => ({
    ...used,
    goesTo: categoryChoices.value[used.category.id] ?? LATER,
  })),
)

/** To step 2, reading the upload's accounts and categories the first time. A new upload starts over. */
async function toAccounts() {
  const current = upload.value
  if (current === null) return
  if (plan.value?.id !== current.id) {
    error.value = ''
    loading.value = true
    try {
      plan.value = await getActualImport(current.id)
    } catch (failure) {
      error.value = failure instanceof Error ? failure.message : String(failure)
      return
    } finally {
      loading.value = false
    }
    accountChoices.value = startAccountChoices(plan.value)
    categoryChoices.value = {}
  }
  step.value = 2
}

/** To step 3, giving each category not chosen yet its pre-filled Goes to. */
async function toCategories() {
  if (plan.value === null) return
  // The pre-fill matches app categories by name, so it waits for them *(agent)*.
  await categories.suspense()
  categoryChoices.value = startCategoryChoices(plan.value, groups.value, categoryChoices.value)
  step.value = 3
}
</script>

<template>
  <div class="flex max-w-3xl flex-col gap-5 p-4 lg:max-w-5xl lg:px-6">
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
          :disabled="upload === null || loading"
          @click="toAccounts"
        >
          Next
        </button>
      </div>
    </section>

    <section
      v-else-if="step === 2 && plan"
      aria-labelledby="accounts-heading"
      class="flex flex-col gap-4"
    >
      <div class="flex flex-col gap-1">
        <h2 id="accounts-heading" class="font-medium">Accounts</h2>
        <p class="text-sm text-muted-foreground">
          Choose what happens to each account in {{ plan.budget_name }}: create it here, treat it as
          one the app has already, or skip it and its rows.
        </p>
      </div>
      <table role="table" class="w-full text-sm max-lg:block">
        <thead class="text-left text-muted-foreground max-lg:hidden">
          <tr class="border-b">
            <th scope="col" class="py-2 pr-3 font-medium">Actual account</th>
            <th scope="col" class="py-2 pr-3 text-right font-medium">Rows</th>
            <th scope="col" class="py-2 pr-3 text-right font-medium">Balance</th>
            <th scope="col" class="py-2 font-medium">What happens</th>
          </tr>
        </thead>
        <tbody role="rowgroup" class="max-lg:block lg:divide-y">
          <ImportAccountRow
            v-for="({ account, choice }, i) in accountRows"
            :key="account.id"
            :model-value="choice"
            :account="account"
            :app-accounts="plan.app_accounts"
            :index="i"
            :problem="problems[account.id]"
            :formatting="formatting"
            @update:model-value="(choice) => (accountChoices[account.id] = choice)"
          />
        </tbody>
      </table>
      <div class="flex justify-between">
        <button type="button" class="rounded-md border px-3 py-2" @click="step = 1">Back</button>
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-2 text-primary-foreground disabled:opacity-50"
          :disabled="Object.keys(problems).length > 0"
          @click="toCategories"
        >
          Next
        </button>
      </div>
    </section>

    <section
      v-else-if="step === 3 && plan"
      aria-labelledby="categories-heading"
      class="flex flex-col gap-4"
    >
      <div class="flex flex-col gap-1">
        <h2 id="categories-heading" class="font-medium">Categories</h2>
        <p class="text-sm text-muted-foreground">
          Choose where each Actual category's rows go; many can go to one. Rows left on Decide later
          come in uncategorized and wait in From Actual, undecided.
        </p>
      </div>
      <p v-if="categoryRows.length === 0" class="rounded-xl border border-dashed p-4 text-sm">
        No imported row has a category.
      </p>
      <table v-else role="table" class="w-full text-sm max-lg:block">
        <thead class="text-left text-muted-foreground max-lg:hidden">
          <tr class="border-b">
            <th scope="col" class="py-2 pr-3 font-medium">Actual category</th>
            <th scope="col" class="py-2 pr-3 font-medium">Group</th>
            <th scope="col" class="py-2 pr-3 text-right font-medium">Rows</th>
            <th scope="col" class="py-2 font-medium">Goes to</th>
          </tr>
        </thead>
        <tbody role="rowgroup" class="max-lg:block lg:divide-y">
          <ImportCategoryRow
            v-for="({ category, rows, goesTo }, i) in categoryRows"
            :key="category.id"
            :model-value="goesTo"
            :category="category"
            :rows="rows"
            :groups="groups"
            :index="i"
            :locale="formatting.locale"
            @update:model-value="(value) => (categoryChoices[category.id] = value)"
          />
        </tbody>
      </table>
      <div class="flex justify-between">
        <button type="button" class="rounded-md border px-3 py-2" @click="step = 2">Back</button>
        <button
          type="button"
          class="rounded-md bg-primary px-3 py-2 text-primary-foreground"
          @click="step = 4"
        >
          Next
        </button>
      </div>
    </section>

    <section v-else class="flex flex-col gap-4">
      <p class="rounded-xl border border-dashed p-4 text-sm text-muted-foreground">
        Importing the accounts and rows you chose comes next, in a later version.
      </p>
      <div class="flex justify-start">
        <button type="button" class="rounded-md border px-3 py-2" @click="step = 3">Back</button>
      </div>
    </section>
  </div>
</template>

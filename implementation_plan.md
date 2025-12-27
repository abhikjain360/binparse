# BinParse Code Generation Implementation Plan

> Always follow ./instructions.md and ./syntax_spec.md

## 1. Project Structure & Dependencies

We will implement the code generator in `binparse-codegen`.

**Dependencies**:

- `proc-macro2`: For token stream generation.
- `quote`: For Rust code templating.
- `binparse-dsl`: To consume the input AST.

## 2. Architecture: Model-View Separation

We will separate the analysis of the AST (Model) from the generation of Rust code (View).

### 2.1 The Model (Analysis Phase)

We will traverse the `binparse-dsl` AST to build an **Analysis Context**.

**Data Structures:**

- `AnalysisContext`: Holds the global symbol table.
- `StructMap`: `HashMap<String, StructAnalysis>` (Using String key for lookup).
- `StructAnalysis`:
  - `is_fixed_size: bool`
  - `fixed_size: Option<usize>` (Total bytes if fixed)
  - `fields: IndexMap<String, FieldAnalysis>` (Preserving definition order, ensuring uniqueness).
  - `is_bit_aligned: bool` (Does it end on a non-byte boundary?)
- `FieldAnalysis`:
  - `is_dynamic: bool`
  - `type_analysis`: Enum distinguishing Primitives, Arrays, Structs, Unions.
  - `size_expression: Option<Expr>` (If size is calculated dynamically).
  - `value_constraint: Option<Expr>` (For constants).
  - `dependencies`: List of other fields this field refers to.

**Analysis Logic:**

1. **Recursion Resolution**: Flatten dependencies. If Struct A contains Struct B, analyze B first.
2. **Unions**: A union is considered **Fixed Size** if and only if all its variants are fixed size and have the exact same size. Otherwise, it is Dynamic.
3. **Duplicates**: Validation step to ensure no duplicate field names exist within a struct.

### 2.2 The View (Generation Phase)

Iterate over the `StructMap` to generate Rust tokens using `quote!`.

## 3. Implementation Details

### 3.A Struct Definition

Generates the Rust struct.

- **Naming**: Use DSL names directly (no case conversion).
- **Fields**:
  - `data: &'a [u8]` (Backing slice, always present).
  - **Nested Structs/Unions**: Store the parsed child instance directly.
    - `field_name: ChildStruct<'a>`
  - **Dynamic Primitives/Arrays**: Store the calculated end offset.
    - `field_name_end: usize`
    - (Optionally `field_name_len` if useful for the getter).
  - **Fixed Primitives/Arrays**: No storage needed (offsets calculated relative to previous fields).

### 3.B The `parse` Method (The Scanner)

**Signature**: `pub fn parse(data: &'a [u8], [ctx_args...]) -> Result<Self, Error>`

**Logic Flow:**

1. **Fast Path (Fixed Size)**:
   - If the struct is Fixed Size and has **no** fields with value constraints (e.g., `magic = 0x1234`) or with dynamic sizing, perform a single check:

     ```rust
     if data.len() < FIXED_SIZE { return Err(Error::UnexpectedEof); }
     ```

   - Return `Ok(Self { data, ...children... })`. (Construct children via trivial parsing/casting logic if needed, or deferred).

2. **Standard Path (Dynamic or Constrained)**:
   - `let mut cursor = 0;`
   - Iterate fields in order:
     - **Constraints**: If field has `value_constraint`, read value at `cursor` and validate.
     - **Nested Structs/Unions**:
       - Call `let child = Child::parse(&data[cursor..])?;`
       - Store `child` in a local variable (to be moved into `Self`).
       - Advance `cursor += child.byte_len();` (Child must expose len).
     - **Dynamic Arrays**:
       - Evaluate size expression.
       - Check bounds.
       - Advance `cursor`.
       - Store `field_end = cursor`.
     - **Bitfields**: Track bit alignment.
3. **Construct**: Return `Ok(Self { ... })`.

### 3.C Getters

Generate methods for each field (except `@skip`).

**Signature**: `pub fn name(&self) -> (Type, usize, usize)` (Value, Offset, Len)
_Note: `Len` is returned for dynamic sized fields/structs/unions._

**Logic**:

- **Nested Structs/Unions**:
  - Return `(self.field_name.clone(), offset, len)`. (Cheap clone of reference-holding struct).
- **Primitives/Arrays**:
  - **Offset**: Recover using `data` start or cached `field_end` from previous field.
  - **Value**:
    - Apply `@transform` if present (passing the raw type, returning target type).
    - Decode (e.g., `u16::from_be_bytes`).
    - Mask/Shift for bitfields.

## 4. Specific Requirement Deep Dive

### 4.1 Transforms (R6.1)

- **Attribute**: `@transform(fn("path::to::func"))`
- **Behavior**: The transform is applied in the **Getter**.
- **Input**: The raw parsed primitive/array (e.g., `[u8; 16]`).
- **Output**: The return type of the function (e.g., `MyKeyType`).

### 4.2 Unions (R1.6)

- **Model**: Analyzed for fixed vs dynamic size.
- **View**:
  - Generate `enum UnionName<'a>`.
  - `parse(data, discriminant)` method matches on discriminant to call specific Variant parser.

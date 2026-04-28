---
name: write-tests
description: Generate tests for existing code. Use when asked to write tests, add test coverage, or create test cases for a function, module, or feature.
---

# Write Tests

## Process

1. **Read the code under test**: Understand the function signatures, inputs, outputs, side effects, and error paths.
2. **Identify test cases**: For each function or behavior, enumerate:
   - Happy path (normal expected usage)
   - Edge cases (empty input, zero, max values, boundary conditions)
   - Error cases (invalid input, missing data, network failures)
   - State transitions (if applicable)
3. **Write tests**: Use the project's existing test framework and patterns. Match the style of any existing tests.
4. **Verify**: Run the tests to confirm they pass. Fix any failures.

## Guidelines

- **One assertion per concept**: Each test should verify one logical behavior. It's fine to have multiple asserts if they all test the same thing.
- **Descriptive names**: Test names should describe the scenario, not the implementation. Use `test_returns_empty_list_when_no_items` over `test_function_1`.
- **No implementation coupling**: Test behavior, not implementation details. Don't assert on internal state unless it's the purpose of the test.
- **Arrange-Act-Assert**: Structure each test as setup, execution, and verification.
- **Isolate side effects**: Mock or stub external dependencies (network, filesystem, database) unless you're writing integration tests.

## Gotchas

- Check if the project has test utilities, fixtures, or factories. Use them instead of creating new ones.
- Look at existing tests for patterns — follow the same style for imports, setup, and assertions.
- Don't test trivial getters/setters or framework behavior. Focus on business logic and edge cases.
- If you can't determine the expected behavior from the code alone, ask rather than guess.

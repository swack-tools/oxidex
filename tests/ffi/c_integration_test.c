/**
 * C FFI Integration Test
 *
 * This test verifies that the C FFI bindings work correctly.
 * It tests basic handle lifecycle, error handling, and metadata operations.
 */

#include "../../include/exiftool_rs.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

/* Test counter */
static int tests_passed = 0;
static int tests_failed = 0;

/* Assertion macro with message */
#define TEST_ASSERT(condition, message) \
    do { \
        if (condition) { \
            tests_passed++; \
            printf("  [PASS] %s\n", message); \
        } else { \
            tests_failed++; \
            printf("  [FAIL] %s\n", message); \
            printf("         Last error: %s\n", exiftool_get_last_error()); \
        } \
    } while (0)

/**
 * Test 1: Handle Lifecycle
 *
 * Verifies that handles can be created and destroyed correctly.
 */
void test_handle_lifecycle() {
    printf("\nTest 1: Handle Lifecycle\n");

    /* Create handle */
    ExifToolHandle* handle = exiftool_create();
    TEST_ASSERT(handle != NULL, "Handle creation succeeds");

    /* Destroy handle */
    exiftool_destroy(handle);
    TEST_ASSERT(1, "Handle destruction succeeds");

    /* Destroying NULL is safe */
    exiftool_destroy(NULL);
    TEST_ASSERT(1, "Destroying NULL handle is safe");
}

/**
 * Test 2: Error Handling
 *
 * Verifies that error codes and messages work correctly.
 */
void test_error_handling() {
    printf("\nTest 2: Error Handling\n");

    ExifToolHandle* handle = exiftool_create();
    TEST_ASSERT(handle != NULL, "Handle created for error tests");

    /* NULL pointer error */
    int result = exiftool_read_file(NULL, "test.jpg");
    TEST_ASSERT(result == EXIFTOOL_ERR_NULL_POINTER, "NULL handle returns error");

    /* File not found error */
    result = exiftool_read_file(handle, "/nonexistent/path/to/file.jpg");
    TEST_ASSERT(result == EXIFTOOL_ERR_IO, "Nonexistent file returns I/O error");

    const char* error = exiftool_get_last_error();
    TEST_ASSERT(error != NULL, "Error message is not NULL");
    TEST_ASSERT(strlen(error) > 0, "Error message is not empty");

    exiftool_destroy(handle);
}

/**
 * Test 3: Tag Operations (Without File I/O)
 *
 * Tests setting and getting tags without reading from a file.
 */
void test_tag_operations() {
    printf("\nTest 3: Tag Operations\n");

    ExifToolHandle* handle = exiftool_create();
    TEST_ASSERT(handle != NULL, "Handle created for tag tests");

    /* Initially empty */
    size_t count = exiftool_get_tag_count(handle);
    TEST_ASSERT(count == 0, "New handle has zero tags");

    /* Set string tag */
    int result = exiftool_set_tag_string(handle, "EXIF:Make", "Test Camera");
    TEST_ASSERT(result == EXIFTOOL_OK, "Set string tag succeeds");

    /* Verify tag exists */
    int exists = exiftool_has_tag(handle, "EXIF:Make");
    TEST_ASSERT(exists == 1, "Tag exists after setting");

    /* Get string tag */
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    TEST_ASSERT(make != NULL, "Get string tag returns non-NULL");
    TEST_ASSERT(strcmp(make, "Test Camera") == 0, "String value is correct");

    /* Set integer tag */
    result = exiftool_set_tag_integer(handle, "EXIF:ISO", 800);
    TEST_ASSERT(result == EXIFTOOL_OK, "Set integer tag succeeds");

    /* Get integer tag */
    int64_t iso;
    result = exiftool_get_tag_integer(handle, "EXIF:ISO", &iso);
    TEST_ASSERT(result == EXIFTOOL_OK, "Get integer tag succeeds");
    TEST_ASSERT(iso == 800, "Integer value is correct");

    /* Set float tag */
    result = exiftool_set_tag_float(handle, "EXIF:FNumber", 2.8);
    TEST_ASSERT(result == EXIFTOOL_OK, "Set float tag succeeds");

    /* Get float tag */
    double fnumber;
    result = exiftool_get_tag_float(handle, "EXIF:FNumber", &fnumber);
    TEST_ASSERT(result == EXIFTOOL_OK, "Get float tag succeeds");
    TEST_ASSERT(fnumber == 2.8, "Float value is correct");

    /* Count should now be 3 */
    count = exiftool_get_tag_count(handle);
    TEST_ASSERT(count == 3, "Tag count is correct after adding tags");

    /* Test tag iteration */
    for (size_t i = 0; i < count; i++) {
        const char* name = exiftool_get_tag_name_at(handle, i);
        TEST_ASSERT(name != NULL, "Tag name at index is non-NULL");
    }

    /* Out of bounds returns NULL */
    const char* invalid = exiftool_get_tag_name_at(handle, 999);
    TEST_ASSERT(invalid == NULL, "Out of bounds index returns NULL");

    /* Remove tag */
    result = exiftool_remove_tag(handle, "EXIF:ISO");
    TEST_ASSERT(result == EXIFTOOL_OK, "Remove tag succeeds");

    exists = exiftool_has_tag(handle, "EXIF:ISO");
    TEST_ASSERT(exists == 0, "Removed tag no longer exists");

    count = exiftool_get_tag_count(handle);
    TEST_ASSERT(count == 2, "Tag count decremented after removal");

    exiftool_destroy(handle);
}

/**
 * Test 4: Type Checking
 *
 * Verifies that type mismatches are caught correctly.
 */
void test_type_checking() {
    printf("\nTest 4: Type Checking\n");

    ExifToolHandle* handle = exiftool_create();
    TEST_ASSERT(handle != NULL, "Handle created for type tests");

    /* Set a string tag */
    exiftool_set_tag_string(handle, "EXIF:Make", "Canon");

    /* Try to get it as an integer (should fail) */
    int64_t value;
    int result = exiftool_get_tag_integer(handle, "EXIF:Make", &value);
    TEST_ASSERT(result == EXIFTOOL_ERR_INVALID_TAG_VALUE, "Type mismatch returns error");

    /* Accessing non-existent tag */
    result = exiftool_get_tag_integer(handle, "EXIF:NonExistent", &value);
    TEST_ASSERT(result == EXIFTOOL_ERR_TAG_NOT_FOUND, "Non-existent tag returns error");

    exiftool_destroy(handle);
}

/**
 * Test 5: NULL Pointer Safety
 *
 * Verifies that NULL pointer handling is correct.
 */
void test_null_pointer_safety() {
    printf("\nTest 5: NULL Pointer Safety\n");

    ExifToolHandle* handle = exiftool_create();
    TEST_ASSERT(handle != NULL, "Handle created for NULL tests");

    /* NULL tag name */
    int result = exiftool_set_tag_string(handle, NULL, "value");
    TEST_ASSERT(result == EXIFTOOL_ERR_NULL_POINTER, "NULL tag name returns error");

    /* NULL value */
    result = exiftool_set_tag_string(handle, "EXIF:Make", NULL);
    TEST_ASSERT(result == EXIFTOOL_ERR_NULL_POINTER, "NULL value returns error");

    /* NULL output pointer */
    result = exiftool_get_tag_integer(handle, "EXIF:ISO", NULL);
    TEST_ASSERT(result == EXIFTOOL_ERR_NULL_POINTER, "NULL output pointer returns error");

    /* NULL handle (get operations should return safe defaults) */
    size_t count = exiftool_get_tag_count(NULL);
    TEST_ASSERT(count == 0, "NULL handle returns zero count");

    int exists = exiftool_has_tag(NULL, "EXIF:Make");
    TEST_ASSERT(exists == 0, "NULL handle returns false for has_tag");

    exiftool_destroy(handle);
}

/**
 * Test 6: Invalid Float Values
 *
 * Verifies that NaN and infinity are rejected.
 */
void test_invalid_float_values() {
    printf("\nTest 6: Invalid Float Values\n");

    ExifToolHandle* handle = exiftool_create();
    TEST_ASSERT(handle != NULL, "Handle created for float tests");

    /* NaN should be rejected */
    int result = exiftool_set_tag_float(handle, "EXIF:FNumber", 0.0 / 0.0);
    TEST_ASSERT(result == EXIFTOOL_ERR_INVALID_TAG_VALUE, "NaN is rejected");

    /* Infinity should be rejected */
    result = exiftool_set_tag_float(handle, "EXIF:FNumber", 1.0 / 0.0);
    TEST_ASSERT(result == EXIFTOOL_ERR_INVALID_TAG_VALUE, "Infinity is rejected");

    /* Normal value should work */
    result = exiftool_set_tag_float(handle, "EXIF:FNumber", 2.8);
    TEST_ASSERT(result == EXIFTOOL_OK, "Normal float value is accepted");

    exiftool_destroy(handle);
}

/**
 * Main test runner
 */
int main(void) {
    printf("========================================\n");
    printf("ExifTool-RS C FFI Integration Tests\n");
    printf("========================================\n");

    /* Run all tests */
    test_handle_lifecycle();
    test_error_handling();
    test_tag_operations();
    test_type_checking();
    test_null_pointer_safety();
    test_invalid_float_values();

    /* Print summary */
    printf("\n========================================\n");
    printf("Test Summary\n");
    printf("========================================\n");
    printf("Passed: %d\n", tests_passed);
    printf("Failed: %d\n", tests_failed);
    printf("Total:  %d\n", tests_passed + tests_failed);

    if (tests_failed == 0) {
        printf("\nAll tests PASSED! ✓\n");
        return 0;
    } else {
        printf("\nSome tests FAILED! ✗\n");
        return 1;
    }
}

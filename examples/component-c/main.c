// Component Model Demo Application (C version)
// This application demonstrates file system operations using standard C I/O.
// It will be composed with the vfs-provider component to use in-memory VFS.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
#include <dirent.h>
#include <errno.h>

// Forward declarations
void test_basic_file_operations(void);
void test_directory_operations(void);
void test_metadata_operations(void);
void test_error_handling(void);

int main(void) {
    printf("=== Component Model VFS Demo (C) ===\n\n");

    // Test 1: Basic file operations
    test_basic_file_operations();

    // Test 2: Directory operations
    test_directory_operations();

    // Test 3: Metadata operations
    test_metadata_operations();

    // Test 4: Error handling
    test_error_handling();

    printf("\n=== All tests completed ===\n");
    return 0;
}

void test_basic_file_operations(void) {
    printf("Test 1: Basic File Operations\n");
    printf("------------------------------\n");

    const char *filename = "/test.txt";
    const char *content = "Hello from Component Model (C)!";
    FILE *fp;
    char buffer[256];

    // Create and write to a file
    fp = fopen(filename, "w");
    if (fp != NULL) {
        fputs(content, fp);
        fclose(fp);
        printf("✓ Created and wrote to %s\n", filename);
    } else {
        printf("✗ Failed to write file: %s\n", strerror(errno));
    }

    // Read the file back
    fp = fopen(filename, "r");
    if (fp != NULL) {
        fgets(buffer, sizeof(buffer), fp);
        fclose(fp);
        if (strcmp(buffer, content) == 0) {
            printf("✓ Read file successfully: '%s'\n", buffer);
        } else {
            printf("✗ Content mismatch: expected '%s', got '%s'\n", content, buffer);
        }
    } else {
        printf("✗ Failed to read file: %s\n", strerror(errno));
    }

    // Append to the file
    fp = fopen(filename, "a");
    if (fp != NULL) {
        fputs("\nAppended line", fp);
        fclose(fp);
        printf("✓ Appended to file\n");
    } else {
        printf("✗ Failed to append: %s\n", strerror(errno));
    }

    // Verify appended content
    fp = fopen(filename, "r");
    if (fp != NULL) {
        int line_count = 0;
        while (fgets(buffer, sizeof(buffer), fp) != NULL) {
            line_count++;
        }
        fclose(fp);
        if (line_count == 2) {
            printf("✓ Append verification successful\n");
        } else {
            printf("✗ Append verification failed (got %d lines)\n", line_count);
        }
    } else {
        printf("✗ Failed to read appended file: %s\n", strerror(errno));
    }

    // Delete the file
    if (unlink(filename) == 0) {
        printf("✓ Deleted %s\n", filename);
    } else {
        printf("✗ Failed to delete file: %s\n", strerror(errno));
    }

    printf("\n");
}

void test_directory_operations(void) {
    printf("Test 2: Directory Operations\n");
    printf("----------------------------\n");

    const char *dirname = "/testdir";

    // Create a directory
    if (mkdir(dirname, 0755) == 0) {
        printf("✓ Created directory %s\n", dirname);
    } else {
        printf("✗ Failed to create directory: %s\n", strerror(errno));
    }

    // Create files in the directory
    const char *files[] = {
        "/testdir/file1.txt",
        "/testdir/file2.txt",
        NULL
    };

    for (int i = 0; files[i] != NULL; i++) {
        FILE *fp = fopen(files[i], "w");
        if (fp != NULL) {
            fprintf(fp, "Content of %s", files[i]);
            fclose(fp);
            printf("✓ Created %s\n", files[i]);
        } else {
            printf("✗ Failed to create %s: %s\n", files[i], strerror(errno));
        }
    }

    // List directory contents
    DIR *dir = opendir(dirname);
    if (dir != NULL) {
        printf("✓ Listing contents of %s:\n", dirname);
        struct dirent *entry;
        while ((entry = readdir(dir)) != NULL) {
            // Skip . and ..
            if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
                continue;
            }
            const char *type = (entry->d_type == DT_DIR) ? "[DIR]" : "[FILE]";
            printf("  %s %s\n", type, entry->d_name);
        }
        closedir(dir);
    } else {
        printf("✗ Failed to read directory: %s\n", strerror(errno));
    }

    // Clean up
    for (int i = 0; files[i] != NULL; i++) {
        unlink(files[i]);
    }
    if (rmdir(dirname) == 0) {
        printf("✓ Cleaned up directory %s\n", dirname);
    } else {
        printf("✗ Failed to remove directory: %s\n", strerror(errno));
    }

    printf("\n");
}

void test_metadata_operations(void) {
    printf("Test 3: Metadata Operations\n");
    printf("---------------------------\n");

    const char *filename = "/metadata_test.txt";
    const char *content = "Test content for metadata";

    // Create file
    FILE *fp = fopen(filename, "w");
    if (fp != NULL) {
        fputs(content, fp);
        fclose(fp);
        printf("✓ Created %s\n", filename);
    } else {
        printf("✗ Failed to create file: %s\n", strerror(errno));
        return;
    }

    // Get metadata
    struct stat st;
    if (stat(filename, &st) == 0) {
        printf("✓ File metadata:\n");
        printf("  Size: %ld bytes\n", (long)st.st_size);
        printf("  Is file: %s\n", S_ISREG(st.st_mode) ? "yes" : "no");
        printf("  Is directory: %s\n", S_ISDIR(st.st_mode) ? "yes" : "no");
    } else {
        printf("✗ Failed to get metadata: %s\n", strerror(errno));
    }

    // Truncate file
    fp = fopen(filename, "w");
    if (fp != NULL) {
        fclose(fp);
        printf("✓ Truncated file\n");
        if (stat(filename, &st) == 0) {
            printf("  New size: %ld bytes\n", (long)st.st_size);
        }
    } else {
        printf("✗ Failed to truncate file: %s\n", strerror(errno));
    }

    // Clean up
    unlink(filename);
    printf("\n");
}

void test_error_handling(void) {
    printf("Test 4: Error Handling\n");
    printf("---------------------\n");

    // Try to read non-existent file
    FILE *fp = fopen("/nonexistent.txt", "r");
    if (fp == NULL) {
        printf("✓ Correctly handled missing file: %s\n", strerror(errno));
    } else {
        printf("✗ Should have failed reading non-existent file\n");
        fclose(fp);
    }

    // Try to remove non-existent file
    if (unlink("/nonexistent.txt") == -1) {
        printf("✓ Correctly handled removing missing file: %s\n", strerror(errno));
    } else {
        printf("✗ Should have failed removing non-existent file\n");
    }

    // Try to create directory that already exists
    const char *dirname = "/test_dup";
    if (mkdir(dirname, 0755) == 0) {
        printf("✓ Created %s\n", dirname);
        if (mkdir(dirname, 0755) == -1) {
            printf("✓ Correctly handled duplicate directory: %s\n", strerror(errno));
        } else {
            printf("✗ Should have failed creating duplicate directory\n");
        }
        rmdir(dirname);
    } else {
        printf("✗ Failed to create test directory: %s\n", strerror(errno));
    }

    // Try to read directory as file
    if (mkdir("/dirtest", 0755) == 0) {
        fp = fopen("/dirtest", "r");
        if (fp == NULL) {
            printf("✓ Correctly handled reading directory as file: %s\n", strerror(errno));
        } else {
            printf("✗ Should have failed reading directory as file\n");
            fclose(fp);
        }
        rmdir("/dirtest");
    } else {
        printf("✗ Failed to create test directory: %s\n", strerror(errno));
    }

    printf("\n");
}

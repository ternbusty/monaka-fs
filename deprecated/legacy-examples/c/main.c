// C Integration Example
// Demonstrates C code using Rust filesystem via FFI

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

// File stat structure (matches Rust FsStat)
struct fs_stat
{
    unsigned long long size;
    unsigned long long created;
    unsigned long long modified;
};

// External filesystem functions provided by fs-wasm
extern int fs_open_path(const char *path, unsigned int path_len);
extern int fs_open_path_with_flags(const char *path, unsigned int path_len, unsigned int flags);
extern int fs_write(unsigned int fd, const char *data, unsigned int data_len);
extern int fs_read(unsigned int fd, char *buffer, unsigned int buffer_len);
extern int fs_close(unsigned int fd);
extern long long fs_seek(unsigned int fd, long long offset, int whence);
extern int fs_fstat(unsigned int fd, struct fs_stat *stat_out);
extern int fs_mkdir(const char *path, unsigned int path_len);

// File open flags (matching Rust definitions)
#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 0x40   // 64
#define O_TRUNC 0x200  // 512
#define O_APPEND 0x400 // 1024

// Test 1: Basic file operations
int test_basic_operations(void)
{
    printf("=== Test 1: Basic File Operations ===\n");

    // Create directory
    const char *dir = "/test";
    if (fs_mkdir(dir, strlen(dir)) != 0)
    {
        printf("FAILED: mkdir\n");
        return 1;
    }
    printf("Created directory: %s\n", dir);

    // Open file
    const char *path = "/test/hello.txt";
    int fd = fs_open_path(path, strlen(path));
    if (fd <= 0)
    {
        printf("FAILED: open file\n");
        return 1;
    }
    printf("Opened file: %s (fd=%d)\n", path, fd);

    // Write data
    const char *content = "Hello from C!";
    int written = fs_write(fd, content, strlen(content));
    if (written != (int)strlen(content))
    {
        printf("FAILED: write\n");
        fs_close(fd);
        return 1;
    }
    printf("Wrote %d bytes: \"%s\"\n", written, content);

    // Get file metadata using fstat
    struct fs_stat stat;
    if (fs_fstat(fd, &stat) != 0)
    {
        printf("FAILED: fstat\n");
        fs_close(fd);
        return 1;
    }
    printf("File size: %llu bytes\n", stat.size);

    // Convert timestamps to ISO 8601 format
    time_t created_time = stat.created;
    struct tm *created_tm = gmtime(&created_time);
    char created_str[32];
    strftime(created_str, sizeof(created_str), "%Y-%m-%dT%H:%M:%SZ", created_tm);

    time_t modified_time = stat.modified;
    struct tm *modified_tm = gmtime(&modified_time);
    char modified_str[32];
    strftime(modified_str, sizeof(modified_str), "%Y-%m-%dT%H:%M:%SZ", modified_tm);

    printf("Created at: %s\n", created_str);
    printf("Modified at: %s\n", modified_str);

    // Seek to beginning
    if (fs_seek(fd, 0, 0) != 0)
    {
        printf("FAILED: seek\n");
        fs_close(fd);
        return 1;
    }

    // Allocate buffer with exact size
    char *buffer = malloc(stat.size + 1);
    if (buffer == NULL)
    {
        printf("FAILED: malloc\n");
        fs_close(fd);
        return 1;
    }
    memset(buffer, 0, stat.size + 1);

    // Read data back
    int read_bytes = fs_read(fd, buffer, stat.size);
    if (read_bytes != (int)stat.size)
    {
        printf("FAILED: read\n");
        free(buffer);
        fs_close(fd);
        return 1;
    }
    printf("Read %d bytes: \"%s\"\n", read_bytes, buffer);

    // Verify content
    if (strcmp(buffer, content) != 0)
    {
        printf("FAILED: content mismatch\n");
        free(buffer);
        fs_close(fd);
        return 1;
    }
    printf("Content verification: OK\n");

    free(buffer);
    fs_close(fd);
    printf("File closed\n\n");
    return 0;
}

// Test 2: Multiple files
int test_multiple_files(void)
{
    printf("=== Test 2: Multiple Files ===\n");

    const char *dir = "/data";
    fs_mkdir(dir, strlen(dir));
    printf("Created directory: %s\n", dir);

    const char *files[] = {"/data/file1.txt", "/data/file2.txt", "/data/file3.txt"};
    const char *contents[] = {"Content 1", "Content 2", "Content 3"};

    // Create and write files
    for (int i = 0; i < 3; i++)
    {
        int fd = fs_open_path(files[i], strlen(files[i]));
        if (fd <= 0)
        {
            printf("FAILED: open file %d\n", i);
            return 1;
        }

        int written = fs_write(fd, contents[i], strlen(contents[i]));
        fs_close(fd);
        printf("Created file: %s (%d bytes)\n", files[i], written);
    }

    // Read and verify
    for (int i = 0; i < 3; i++)
    {
        int fd = fs_open_path(files[i], strlen(files[i]));
        char buffer[100] = {0};
        int read_bytes = fs_read(fd, buffer, sizeof(buffer) - 1);
        fs_close(fd);

        if (strcmp(buffer, contents[i]) != 0)
        {
            printf("FAILED: content mismatch in file %d\n", i);
            return 1;
        }
        printf("Verified file %d: \"%s\" (%d bytes)\n", i + 1, buffer, read_bytes);
    }

    printf("\n");
    return 0;
}

// Test 3: Large file
int test_large_file(void)
{
    printf("=== Test 3: Large File (16KB) ===\n");

    const char *dir = "/large";
    fs_mkdir(dir, strlen(dir));
    printf("Created directory: %s\n", dir);

    const char *path = "/large/test.bin";
    int fd = fs_open_path(path, strlen(path));
    if (fd <= 0)
    {
        printf("FAILED: open file\n");
        return 1;
    }
    printf("Opened file: %s (fd=%d)\n", path, fd);

    // Write 16KB of data
    const int size = 16384;
    char write_buffer[16384];
    char read_buffer[16384];

    for (int i = 0; i < size; i++)
    {
        write_buffer[i] = i % 256;
    }

    int written = fs_write(fd, write_buffer, size);
    if (written != size)
    {
        printf("FAILED: write\n");
        fs_close(fd);
        return 1;
    }
    printf("Wrote %d bytes (16KB pattern)\n", written);

    // Read back
    fs_seek(fd, 0, 0);
    memset(read_buffer, 0, size);

    int read_bytes = fs_read(fd, read_buffer, size);
    if (read_bytes != size)
    {
        printf("FAILED: read\n");
        fs_close(fd);
        return 1;
    }
    printf("Read %d bytes\n", read_bytes);

    // Verify
    if (memcmp(write_buffer, read_buffer, size) != 0)
    {
        printf("FAILED: data mismatch\n");
        fs_close(fd);
        return 1;
    }
    printf("Data verification: OK\n");

    fs_close(fd);
    printf("File closed\n\n");
    return 0;
}

// Test 4: Sparse file
int test_sparse_file(void)
{
    printf("=== Test 4: Sparse File ===\n");

    const char *dir = "/sparse";
    fs_mkdir(dir, strlen(dir));
    printf("Created directory: %s\n", dir);

    const char *path = "/sparse/test.dat";
    int fd = fs_open_path(path, strlen(path));
    if (fd <= 0)
    {
        printf("FAILED: open file\n");
        return 1;
    }
    printf("Opened file: %s (fd=%d)\n", path, fd);

    // Write at beginning
    const char *start = "START";
    int written = fs_write(fd, start, strlen(start));
    printf("Wrote \"%s\" at offset 0 (%d bytes)\n", start, written);

    // Seek to large offset
    fs_seek(fd, 32000, 0);
    printf("Seeked to offset 32000\n");

    // Write at end
    const char *end = "END";
    written = fs_write(fd, end, strlen(end));
    printf("Wrote \"%s\" at offset 32000 (%d bytes)\n", end, written);

    // Verify beginning
    fs_seek(fd, 0, 0);
    char buffer[10] = {0};
    fs_read(fd, buffer, 5);
    if (strcmp(buffer, start) != 0)
    {
        printf("FAILED: start mismatch\n");
        fs_close(fd);
        return 1;
    }
    printf("Verified start: \"%s\"\n", buffer);

    // Verify sparse region (should be zeros)
    fs_seek(fd, 1000, 0);
    memset(buffer, 0xFF, 5);
    int read_bytes = fs_read(fd, buffer, 5);
    for (int i = 0; i < read_bytes; i++)
    {
        if (buffer[i] != 0)
        {
            printf("FAILED: sparse region not zero\n");
            fs_close(fd);
            return 1;
        }
    }
    printf("Verified sparse region at offset 1000: all zeros\n");

    // Verify end
    fs_seek(fd, 32000, 0);
    memset(buffer, 0, sizeof(buffer));
    fs_read(fd, buffer, 3);
    if (strcmp(buffer, end) != 0)
    {
        printf("FAILED: end mismatch\n");
        fs_close(fd);
        return 1;
    }
    printf("Verified end: \"%s\"\n", buffer);

    fs_close(fd);
    printf("File closed\n\n");
    return 0;
}

// Test 5: O_APPEND operations
int test_append_operations(void)
{
    printf("=== Test 5: O_APPEND Operations ===\n");

    const char *path = "/append_test.txt";

    // Create file with initial content
    int fd = fs_open_path_with_flags(path, strlen(path), O_RDWR | O_CREAT);
    if (fd <= 0)
    {
        printf("FAILED: open file\n");
        return 1;
    }
    fs_write(fd, "Initial", 7);
    fs_close(fd);
    printf("Created file with 'Initial' content\n");

    // Open in append mode
    fd = fs_open_path_with_flags(path, strlen(path), O_WRONLY | O_APPEND);
    if (fd <= 0)
    {
        printf("FAILED: open file in append mode\n");
        return 1;
    }

    // Write multiple times - all should append
    fs_write(fd, " First", 6);
    fs_write(fd, " Second", 7);
    fs_write(fd, " Third", 6);
    printf("Appended three strings\n");

    fs_close(fd);

    // Verify content
    fd = fs_open_path_with_flags(path, strlen(path), O_RDONLY);
    if (fd <= 0)
    {
        printf("FAILED: open file for reading\n");
        return 1;
    }

    char buffer[100] = {0};
    int read_bytes = fs_read(fd, buffer, sizeof(buffer) - 1);
    if (read_bytes < 0)
    {
        printf("FAILED: read\n");
        fs_close(fd);
        return 1;
    }

    printf("Final content: '%s'\n", buffer);

    if (strcmp(buffer, "Initial First Second Third") != 0)
    {
        printf("FAILED: content mismatch\n");
        fs_close(fd);
        return 1;
    }

    fs_close(fd);
    printf("Append verification: OK\n\n");
    return 0;
}

// Test 6: O_TRUNC operations
int test_trunc_operations(void)
{
    printf("=== Test 6: O_TRUNC Operations ===\n");

    const char *path = "/trunc_test.txt";

    // Create file with old content
    int fd = fs_open_path_with_flags(path, strlen(path), O_RDWR | O_CREAT);
    if (fd <= 0)
    {
        printf("FAILED: create file\n");
        return 1;
    }
    const char *old_content = "This is very long old content that should be truncated";
    fs_write(fd, old_content, strlen(old_content));
    fs_close(fd);
    printf("Created file with old content (%lu bytes)\n", strlen(old_content));

    // Open with O_TRUNC
    fd = fs_open_path_with_flags(path, strlen(path), O_RDWR | O_TRUNC);
    if (fd <= 0)
    {
        printf("FAILED: open file with O_TRUNC\n");
        return 1;
    }
    printf("Opened file with O_TRUNC flag\n");

    // File should be empty now
    char buffer[100] = {0};
    int read_bytes = fs_read(fd, buffer, sizeof(buffer) - 1);
    if (read_bytes != 0)
    {
        printf("FAILED: expected empty file, but read %d bytes\n", read_bytes);
        fs_close(fd);
        return 1;
    }
    printf("Verified file is empty after O_TRUNC\n");

    // Write new content
    const char *new_content = "New content";
    fs_write(fd, new_content, strlen(new_content));
    printf("Wrote new content: '%s'\n", new_content);

    // Read back
    fs_seek(fd, 0, 0);
    memset(buffer, 0, sizeof(buffer));
    read_bytes = fs_read(fd, buffer, sizeof(buffer) - 1);
    if (read_bytes < 0)
    {
        printf("FAILED: read\n");
        fs_close(fd);
        return 1;
    }

    if (strcmp(buffer, new_content) != 0)
    {
        printf("FAILED: content mismatch: expected '%s', got '%s'\n", new_content, buffer);
        fs_close(fd);
        return 1;
    }

    fs_close(fd);
    printf("Truncate and write verification: OK\n\n");
    return 0;
}

int main(void)
{
    printf("C Integration Example\n");
    printf("=====================\n");
    printf("Demonstrates C code using Rust filesystem via FFI\n\n");

    int failed = 0;
    failed += test_basic_operations();
    failed += test_multiple_files();
    failed += test_large_file();
    failed += test_sparse_file();
    failed += test_append_operations();
    failed += test_trunc_operations();

    if (failed == 0)
    {
        printf("All operations completed successfully!\n");
        return 0;
    }
    else
    {
        printf("%d test(s) failed\n", failed);
        return 1;
    }
}

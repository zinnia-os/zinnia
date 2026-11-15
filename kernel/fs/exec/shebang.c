// SPDX-License-Identifier: BSD-3-Clause

#include <fs/exec/exec.h>
#include <fs/vfs.h>
#include <lib/initgraph.h>
#include <lib/process.h>
#include <mem/alloc.h>
#include <mem/vmo.h>
#include <string.h>
#include <zinnia/limits.h>

constexpr size_t SHEBANG_MAX_DEPTH = 4;

static bool shebang_identify(struct vmo* obj) {
    char buffer[2];
    size_t read;
    if (vmo_read(obj, 0, buffer, sizeof(buffer), &read) || read != sizeof(buffer))
        return false;

    return buffer[0] == '#' && buffer[1] == '!';
}

static bool is_arg_start(const char* line, size_t i) {
    return line[i] != '\0' && (i == 0 || line[i - 1] == '\0');
}

static errno_t shebang_load(struct process* proc, struct exec_info* info, uintptr_t* out_ip, uintptr_t* out_sp) {
    if (info->depth >= SHEBANG_MAX_DEPTH)
        return ELOOP;

    char* interp = mem_alloc(__PATH_MAX + 1, 0);
    if (!interp)
        return ENOMEM;

    size_t len;
    errno_t status = vmo_read(info->file_obj, 2, interp, __PATH_MAX, &len); // Skip the #!
    if (status && len == 0)
        goto free_interp;
    for (size_t i = 0; i < len; i++) {
        if (interp[i] == '\n') {
            len = i;
            break;
        }
    }
    interp[len] = '\0';
    len = strlen(interp);

    // Parse the shebang command line.
    size_t num_interp_args = 0;
    for (size_t i = 0; i < len; i++) {
        if (interp[i] == ' ')
            interp[i] = '\0';
    }
    for (size_t i = 0; i < len; i++) {
        if (is_arg_start(interp, i))
            num_interp_args++;
    }

    size_t num_argv = 0;
    for (const char** ptr = info->argv; *ptr != nullptr; ptr++)
        num_argv++;
    const size_t num_script_args = num_argv ? num_argv - 1 : 0;

    const char** args = mem_alloc((num_interp_args + 1 + num_script_args + 1) * sizeof(*args), 0);
    if (!args) {
        status = ENOMEM;
        goto free_interp;
    }

    // Insert the interpreter path at the start of argv.
    size_t argc = 0;
    for (size_t i = 0; i < len; i++) {
        if (is_arg_start(interp, i))
            args[argc++] = &interp[i];
    }
    args[argc++] = info->exec_path;
    for (size_t i = 0; i < num_script_args; i++)
        args[argc++] = info->argv[i + 1];
    args[argc] = nullptr;

    const char* interp_path = args[0];

    struct vmo* object;
    status = exec_open(proc, interp_path, &object);
    if (status)
        goto free_args;

    struct exec_format* format = exec_identify(object);
    if (!format) {
        status = ENOEXEC;
        goto unref_object;
    }

    // Load the interpreted file with this one as the interpreter.
    struct exec_info interp_info = {
        .space = info->space,
        .file_obj = object,
        .exec_path = info->exec_path,
        .argv = args,
        .envp = info->envp,
        .depth = info->depth + 1,
    };
    status = format->load(proc, &interp_info, out_ip, out_sp);

unref_object:
    vmo_unref(object);
free_args:
    mem_free(args, (num_interp_args + 1 + num_script_args + 1) * sizeof(*args));
free_interp:
    mem_free(interp, __PATH_MAX + 1);
    return status;
}

static struct exec_format shebang_format = {
    .name = "shebang",
    .identify = shebang_identify,
    .load = shebang_load,
};

static void shebang_stage_init(struct boot_info*) {
    exec_register(&shebang_format);
}
INIT_TASK(shebang_stage, "fs.exec.shebang", shebang_stage_init);
INIT_DEPENDS(shebang_stage, mem_stage);
INIT_ENTAILS(shebang_stage, vfs_stage);

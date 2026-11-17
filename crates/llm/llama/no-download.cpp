// llama.cpp's common/download.cpp and common/hf-cache.cpp for Lastbook's local AI runner: the same functions, without the
// network. The runner never fetches a model or anything else; these answer that nothing can be fetched and that there is
// no download cache. Built in their place by no-download.cmake.

#include "download.h"
#include "hf-cache.h"

std::pair<long, std::vector<char>> common_remote_get_content(const std::string &, const common_remote_params &) {
    return {-1, {}};
}

std::pair<std::string, std::string> common_download_split_repo_tag(const std::string & hf_repo_with_tag) {
    const auto colon = hf_repo_with_tag.find(':');
    if (colon == std::string::npos) {
        return {hf_repo_with_tag, "latest"};
    }
    return {hf_repo_with_tag.substr(0, colon), hf_repo_with_tag.substr(colon + 1)};
}

void common_download_run_tasks(const std::vector<common_download_task> & tasks) {
    for (const auto & task : tasks) {
        if (task.opts.callback) {
            common_download_progress p;
            p.url = task.url;
            task.opts.callback->on_done(p, false);
        }
    }
}

std::vector<std::string> common_download_get_all_parts(const std::string & url) {
    return {url};
}

std::vector<common_cached_model_info> common_list_cached_models() {
    return {};
}

std::string common_download_resolve_path(const std::string &, const std::string &) {
    return {};
}

int common_download_file_single(const std::string &, const std::string &, const common_download_opts &, bool) {
    return -1;
}

std::string common_docker_resolve_model(const std::string &) {
    return {};
}

bool common_download_remove(const std::string &) {
    return false;
}

common_download_hf_plan common_download_get_hf_plan(const common_params_model &, const common_download_opts &) {
    return {};
}

namespace hf_cache {

hf_files get_repo_files(const std::string &, const std::string &) {
    return {};
}

hf_files get_cached_files(const std::string &) {
    return {};
}

std::string finalize_file(const hf_file &) {
    return {};
}

bool remove_cached_repo(const std::string &) {
    return false;
}

std::string get_cache_path() {
    return {};
}

} // namespace hf_cache

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * Docker 本地部署管理模块
 *
 * 通过调用 Tauri 后端的 docker CLI 封装命令，提供 Docker 容器的
 * 完整生命周期管理：镜像拉取、容器启停、一键部署创建 API 配置。
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settings";

// ============ 类型定义 ============

export interface DockerStatus {
  installed: boolean;
  running: boolean;
  version: string | null;
}

export interface DockerContainer {
  id: string;
  name: string;
  image: string;
  status: string;
  ports: string;
  created: string;
  /** "running" | "exited" | "created" | ... */
  state: string;
}

export interface DockerImage {
  id: string;
  repository: string;
  tag: string;
  size: string;
  created: string;
}

export interface DockerProfile {
  id: string;
  name: string;
  image: string;
  description: string;
  ports: string[];
  volumes: string[];
  apiUrl: string;
  /** "ollama" | "openai" */
  apiType: string;
  gpu: boolean;
}

export interface DockerPullProgress {
  image: string;
  /** "starting" | "pulling" | "completed" | "failed" */
  status: string;
  message: string;
}

// ============ Store ============

export const useDockerStore = defineStore("docker", () => {
  // ============ 响应式状态 ============

  const dockerStatus = ref<DockerStatus>({
    installed: false,
    running: false,
    version: null,
  });

  const profiles = ref<DockerProfile[]>([]);
  const containers = ref<DockerContainer[]>([]);
  const images = ref<DockerImage[]>([]);

  const isPulling = ref(false);
  const pullingImage = ref("");
  /** 拉取日志行 (最多保留 200 行) */
  const pullLogs = ref<string[]>([]);
  const pullCompleted = ref(false);
  const pullFailed = ref(false);

  const isStartingContainer = ref(false);
  const isLoadingContainers = ref(false);
  const isLoadingImages = ref(false);

  let unlistenPull: UnlistenFn | null = null;

  // ============ 计算属性 ============

  /** 当前是否有 Docker 可用 */
  const isAvailable = computed(
    () => dockerStatus.value.installed && dockerStatus.value.running
  );

  /** 仅本应用创建的容器（名称以 baiyu- 开头） */
  const appContainers = computed(() =>
    containers.value.filter((c) =>
      c.name.replace(/^\//, "").startsWith("baiyu-")
    )
  );

  // ============ 方法 ============

  /** 检查 Docker 安装和守护进程状态 */
  const checkStatus = async () => {
    try {
      dockerStatus.value = await invoke<DockerStatus>("check_docker_status");
    } catch (error) {
      console.error("Failed to check Docker status:", error);
      dockerStatus.value = { installed: false, running: false, version: null };
    }
  };

  /** 加载预设部署方案列表 */
  const loadProfiles = async () => {
    try {
      profiles.value = await invoke<DockerProfile[]>("get_docker_profiles_cmd");
    } catch (error) {
      console.error("Failed to load Docker profiles:", error);
    }
  };

  /** 加载容器列表 (包含已停止容器) */
  const loadContainers = async () => {
    isLoadingContainers.value = true;
    try {
      containers.value = await invoke<DockerContainer[]>(
        "list_docker_containers",
        { all: true }
      );
    } catch (error) {
      console.error("Failed to list Docker containers:", error);
      containers.value = [];
    } finally {
      isLoadingContainers.value = false;
    }
  };

  /** 加载本地已有的 Docker 镜像列表 */
  const loadImages = async () => {
    isLoadingImages.value = true;
    try {
      images.value = await invoke<DockerImage[]>("list_docker_images");
    } catch (error) {
      console.error("Failed to list Docker images:", error);
      images.value = [];
    } finally {
      isLoadingImages.value = false;
    }
  };

  /** 注册拉取进度事件监听器 */
  const setupPullListener = async () => {
    if (unlistenPull) unlistenPull();
    unlistenPull = await listen<DockerPullProgress>(
      "docker-pull-progress",
      (event) => {
        const { status, message } = event.payload;
        pullLogs.value.push(message);
        if (pullLogs.value.length > 200) pullLogs.value.shift();
        if (status === "completed") pullCompleted.value = true;
        if (status === "failed") pullFailed.value = true;
      }
    );
  };

  /** 拉取 Docker 镜像，拉取过程通过事件实时推送日志 */
  const pullImage = async (image: string) => {
    if (isPulling.value) return;

    isPulling.value = true;
    pullingImage.value = image;
    pullLogs.value = [];
    pullCompleted.value = false;
    pullFailed.value = false;

    try {
      await setupPullListener();
      await invoke("pull_docker_image", { image });
      await loadImages();
    } catch (error) {
      console.error("Failed to pull Docker image:", error);
      throw error;
    } finally {
      isPulling.value = false;
      pullingImage.value = "";
      if (unlistenPull) {
        unlistenPull();
        unlistenPull = null;
      }
    }
  };

  /**
   * 启动某个部署方案对应的容器。
   * - 若名为 baiyu-{profileId} 的容器已存在（停止状态），则直接 start
   * - 否则 docker run 创建并启动
   */
  const startContainer = async (profileId: string, containerName?: string) => {
    isStartingContainer.value = true;
    try {
      await invoke<string>("start_docker_container", {
        profileId,
        containerName: containerName ?? undefined,
      });
      await loadContainers();
    } finally {
      isStartingContainer.value = false;
    }
  };

  /** 停止容器 */
  const stopContainer = async (containerId: string) => {
    await invoke("stop_docker_container", { containerId });
    await loadContainers();
  };

  /** 强制删除容器（运行中或停止的均可） */
  const removeContainer = async (containerId: string) => {
    await invoke("remove_docker_container", { containerId });
    await loadContainers();
  };

  /**
   * 判断某个镜像是否已在本地存在。
   * image 格式为 "repo:tag" 或 "repo/name:tag"
   */
  const isImagePulled = (image: string): boolean => {
    const colonIdx = image.lastIndexOf(":");
    const repo = colonIdx >= 0 ? image.slice(0, colonIdx) : image;
    const tag = colonIdx >= 0 ? image.slice(colonIdx + 1) : "latest";
    return images.value.some(
      (img) => img.repository === repo && img.tag === tag
    );
  };

  /**
   * 找到某个方案当前对应的容器（按名称 baiyu-{profileId} 或镜像前缀匹配）。
   */
  const getProfileContainer = (profileId: string): DockerContainer | null => {
    const profile = profiles.value.find((p) => p.id === profileId);
    if (!profile) return null;
    const expectedName = `baiyu-${profileId}`;
    const repoPrefix = profile.image.split(":")[0];
    return (
      containers.value.find(
        (c) =>
          c.name === expectedName ||
          c.name === `/${expectedName}` ||
          c.image.startsWith(repoPrefix)
      ) ?? null
    );
  };

  /**
   * 一键部署：
   * 1. 若镜像不在本地，先拉取
   * 2. 启动容器
   * 3. 在设置中创建对应 API 配置（避免重复）
   */
  const oneClickDeploy = async (profileId: string): Promise<string> => {
    const profile = profiles.value.find((p) => p.id === profileId);
    if (!profile) throw new Error(`未知 Docker 部署方案: ${profileId}`);

    if (!isImagePulled(profile.image)) {
      await pullImage(profile.image);
    }

    await startContainer(profileId);

    const settings = useSettingsStore();
    const existing = settings.apiConfigs.find(
      (c) => c.provider === "local" && c.baseUrl === profile.apiUrl
    );
    if (!existing) {
      settings.createApiConfig(
        `docker-${profileId}`,
        "local",
        "",
        "",
        profile.apiUrl
      );
    }

    return profile.apiUrl;
  };

  // ============ 公共接口 ============

  return {
    // 状态
    dockerStatus,
    profiles,
    containers,
    images,
    isPulling,
    pullingImage,
    pullLogs,
    pullCompleted,
    pullFailed,
    isStartingContainer,
    isLoadingContainers,
    isLoadingImages,

    // 计算属性
    isAvailable,
    appContainers,

    // 方法
    checkStatus,
    loadProfiles,
    loadContainers,
    loadImages,
    pullImage,
    startContainer,
    stopContainer,
    removeContainer,
    isImagePulled,
    getProfileContainer,
    oneClickDeploy,
  };
});

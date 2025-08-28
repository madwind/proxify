package service

import (
	"context"
	"errors"
	"io"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"time"
)

// GetStartTimeFromStream 将流写入临时文件并调用 ffprobe 获取 start_time
func GetStartTimeFromStream(body io.Reader) (float64, error) {
	tmpFile, err := os.CreateTemp("", "tsfile-*.ts")
	if err != nil {
		return 0, err
	}
	defer os.Remove(tmpFile.Name())

	_, err = io.Copy(tmpFile, body)
	if err != nil {
		tmpFile.Close()
		return 0, err
	}
	tmpFile.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "ffprobe",
		"-v", "error",
		"-show_entries", "format=start_time",
		"-of", "default=noprint_wrappers=1:nokey=1",
		tmpFile.Name(),
	)

	out, err := cmd.Output()
	if err != nil {
		return 0, err
	}

	outStr := strings.TrimSpace(string(out))
	if outStr == "" || outStr == "N/A" {
		return 0, errors.New("ffprobe failed to get start_time")
	}

	return strconv.ParseFloat(outStr, 64)
}
